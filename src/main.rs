#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
#![feature(windows_process_extensions_async_pipes)]

use std::collections::VecDeque;
use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;
use std::{fs, io};

use async_std::io::prelude::BufReadExt;
use async_std::process::{Child, Command as Cmd, ExitStatus};
use iced::futures::channel::mpsc;
use iced::futures::io::BufReader;
use iced::futures::sink::SinkExt;
use iced::widget::{button, checkbox, column, radio, row, scrollable, text, text_input, Space};
use iced::window::Settings as WindowSettings;
use iced::{
    executor, theme, Alignment, Application, Color, Command, Element, Length, Settings,
    Subscription, Theme,
};

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
enum Format {
    #[default]
    Png,
    Jpg,
    Webp,
}

pub fn main() -> iced::Result {
    RealEsrgan::run(Settings {
        id: Some("dev.hch12907.realesrgan-ncnn-vulkan-gui".into()),
        window: WindowSettings {
            size: (800, 500),
            ..Default::default()
        },
        ..Default::default()
    })
}

#[derive(Default)]
struct RealEsrgan {
    start_button_text: String,
    input: String,
    output: String,
    current_page: Page,
    upscale_ratio: UpscaleRatio,
    tta_mode: bool,
    advanced_options: bool,
    gpu_id: String,
    model_name: String,
    model_path: String,
    format: Format,
    filename_format: String,

    checker: Option<mpsc::Sender<CheckerTask>>,
    log: VecDeque<String>,
    processing: bool,

    state: RealEsrganState,
}

#[derive(Default)]
struct RealEsrganState {
    selected_files: Vec<OsString>,
    output_dir: OsString,
}

#[derive(Debug, Clone, Copy)]
enum PathType {
    Input = 0,
    Output = 1,
}

#[derive(Debug, Clone, Copy, Default)]
enum Page {
    #[default]
    Processing,
    Output,
    Log,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum UpscaleRatio {
    One = 1,
    Two = 2,
    Three = 3,
    #[default]
    Four = 4,
}

#[derive(Debug, Clone)]
enum Message {
    AdvancedOptionsClicked(bool),
    AskPath { path_type: PathType },
    GpuIdChanged(String),
    ModelPathChanged(String),
    ModelNameChanged(String),
    OutputFormatChanged(Format),
    OutputNameChanged(String),
    PathChanged { path_type: PathType, path: String },
    StartClicked,
    CheckerReady(mpsc::Sender<CheckerTask>),
    ChildUpdate(CheckerResult),
    SwitchPage(Page),
    Tick,
    TTAModeClicked(bool),
    UpscaleRatioSelected(UpscaleRatio),
}

#[derive(Debug)]
enum CheckerStatus {
    Starting,
    Ready(Vec<Child>, mpsc::Receiver<CheckerTask>),
}

#[derive(Clone, Debug)]
enum CheckerResult {
    Ended,
    ChildLog(u32, String),        // (pid, message)
    ChildExited(u32, ExitStatus), // (pid, error_code)
    ChildErrored(u32, String),    // (pid, error)
    SpawnError(String),
}

enum CheckerTask {
    NewChild {
        input_path: OsString,
        output_path: OsString,
        upscale_ratio: u32,
        gpu_id: String,
        model_path: String,
        model_name: String,
        tta_mode: bool,
    },
    Poll,
}

impl RealEsrgan {
    fn add_input_paths(&mut self, path: String) -> Result<(), String> {
        let path = PathBuf::from(path);

        if path.is_dir() {
            let dir = fs::read_dir(path).map_err(|e| e.to_string())?;
            let files = dir.into_iter().filter_map(|entry_res| {
                let path = entry_res.map(|entry| entry.path()).ok()?;
                let extension = path.extension()?.to_string_lossy();

                if path.is_file()
                    && ["png", "jpg", "jpeg", "webp"]
                        .iter()
                        .any(|&ext| ext == extension.to_ascii_lowercase())
                {
                    Some(path)
                } else {
                    None
                }
            });

            for file in files {
                self.state.selected_files.push(file.into())
            }
        } else if path.is_file() {
            self.state.selected_files.push(path.into())
        };

        Ok(())
    }

    fn add_output_path(&mut self, path: String) -> Result<(), String> {
        let path = PathBuf::from(path);

        if path.exists() && path.is_dir() {
            self.state.output_dir = path.into();
            Ok(())
        } else {
            let err = io::Error::new(
                io::ErrorKind::NotFound,
                path.to_str().unwrap_or_default().to_owned(),
            );
            Err(err.to_string())
        }
    }

    fn generate_output_filename(
        format: &str,
        input: PathBuf,
        ratio: UpscaleRatio,
        model: &str,
    ) -> Result<OsString, String> {
        enum FileFormat {
            Name,
            Scale,
            Model,
            Other(String),
        }

        use FileFormat::*;

        let mut format_iter = format.chars();
        let mut format_parsed = Vec::new();

        while let Some(f) = format_iter.next() {
            match f {
                '{' if format_iter.as_str().starts_with("name}") => {
                    format_parsed.push(FileFormat::Name);
                    format_iter.by_ref().take("{name}".len() - 1).for_each(drop)
                }

                '{' if format_iter.as_str().starts_with("scale}") => {
                    format_parsed.push(FileFormat::Scale);
                    format_iter
                        .by_ref()
                        .take("{scale}".len() - 1)
                        .for_each(drop)
                }

                '{' if format_iter.as_str().starts_with("model}") => {
                    format_parsed.push(FileFormat::Model);
                    format_iter
                        .by_ref()
                        .take("{model}".len() - 1)
                        .for_each(drop)
                }

                c => {
                    if let Some(i) = format_iter.as_str().find('{') {
                        let text = Some(c)
                            .into_iter()
                            .chain(format_iter.by_ref().take(i))
                            .collect::<String>();
                        format_parsed.push(FileFormat::Other(text))
                    } else {
                        let text = Some(c)
                            .into_iter()
                            .chain(format_iter.by_ref())
                            .collect::<String>();
                        format_parsed.push(FileFormat::Other(text))
                    }
                }
            }
        }

        let Some(filename) = input.file_stem() else {
            Err("Invalid input file selected.")?
        };

        // If OsString is internally u8 units
        #[cfg(not(target_os = "windows"))]
        {
            use std::os::unix::ffi::{OsStrExt, OsStringExt};

            let mut output_filename = Vec::new();

            for fmt in format_parsed {
                match fmt {
                    Name => output_filename.extend(filename.as_bytes()),
                    Scale => output_filename.push(b'0' + ratio as u8),
                    Model => output_filename.extend(model.as_bytes()),
                    Other(s) => output_filename.extend(s.as_bytes()),
                }
            }

            Ok(OsString::from_vec(output_filename))
        }

        // If OsString is internally u16 units
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::ffi::{OsStrExt, OsStringExt};

            let mut output_filename = Vec::new();

            for fmt in format_parsed {
                match fmt {
                    Name => output_filename.extend(filename.encode_wide()),
                    Scale => output_filename.push(b'0' as u16 + ratio as u16),
                    Model => output_filename.extend(model.encode_utf16()),
                    Other(s) => output_filename.extend(s.encode_utf16()),
                }
            }

            Ok(OsString::from_wide(output_filename.as_ref()))
        }
    }

    fn reset_start_button(&mut self) {
        self.start_button_text.clear();
        self.start_button_text.push_str("Click Here to Start");
    }

    fn show_error_on_start_button(&mut self, err: &str) {
        self.start_button_text.clear();
        self.start_button_text
            .push_str(&format!("Click Here to Start ({})", err));
    }

    fn start(&mut self) {
        let error_dialog = |msg| {
            rfd::MessageDialog::new()
                .set_buttons(rfd::MessageButtons::Ok)
                .set_title("Error")
                .set_description(msg)
                .set_level(rfd::MessageLevel::Error)
                .show()
        };

        let ask = |msg| {
            rfd::MessageDialog::new()
                .set_buttons(rfd::MessageButtons::YesNo)
                .set_title("Output Path Selection")
                .set_description(msg)
                .set_level(rfd::MessageLevel::Info)
                .show()
        };

        self.reset_start_button();

        if self.state.selected_files.is_empty() {
            match self.add_input_paths(self.input.clone()) {
                Ok(()) => (),
                Err(msg) => return self.show_error_on_start_button(&msg),
            }
        }

        if self.state.output_dir.is_empty() {
            if self.output.is_empty() {
                let ok = ask(concat!(
                    "An output path is required to proceed.\n\n",
                    "Do you want to use the input directory as the output path?"
                ));

                if ok {
                    self.state.output_dir = PathBuf::from(self.input.clone())
                        .parent()
                        .map(|p| OsString::from(p.to_path_buf()))
                        .unwrap_or_default();

                    if !self.state.output_dir.is_empty() {
                        self.output = self.state.output_dir.to_string_lossy().to_string();
                    } else {
                        error_dialog("Unable to obtain an output directory.");
                        return;
                    }
                } else {
                    return;
                }
            }
            match self.add_output_path(self.output.clone()) {
                Ok(()) => (),
                Err(msg) => return self.show_error_on_start_button(&msg),
            }
        } else if !PathBuf::from(&self.state.output_dir).exists() {
            error_dialog("Invalid output directory.");
            return;
        }

        if self.upscale_ratio as u32 == 1 {
            error_dialog("An upscale ratio greater than 1 is not specified.");
            return;
        }

        let model_name = if self.model_name.is_empty() {
            "realesrgan-x4plus-anime"
        } else {
            &self.model_name
        };

        if (self.upscale_ratio as u32) < 4 && model_name.contains("realesrgan-x4plus") {
            let keep_going = ask(concat!(
                "The upscale ratio is possibly incompatible with the model.\n",
                "The output may possibly be distorted.\n\n",
                "Do you wish to continue?"
            ));

            if !keep_going {
                return;
            }
        }

        for f in self.state.selected_files.iter() {
            let input = PathBuf::from(f);
            let mut output = PathBuf::from(&self.state.output_dir);

            let output_ext = match self.format {
                Format::Png => "png",
                Format::Jpg => "jpg",
                Format::Webp => "webp",
            };

            let filename = match Self::generate_output_filename(
                &self.filename_format,
                input,
                self.upscale_ratio,
                &model_name,
            ) {
                Ok(f) => f,
                Err(e) => {
                    error_dialog(&e);
                    return;
                }
            };

            output.push(&filename);
            output.set_extension(output_ext);

            let sent = self
                .checker
                .as_mut()
                .unwrap()
                .start_send(CheckerTask::NewChild {
                    input_path: f.clone(),
                    output_path: output.into_os_string(),
                    upscale_ratio: self.upscale_ratio as u32,
                    gpu_id: self.gpu_id.clone(),
                    model_path: self.model_path.clone(),
                    model_name: self.model_name.clone(),
                    tta_mode: self.tta_mode,
                });

            if sent.is_err() {
                let err = sent.unwrap_err().to_string();
                let err = format!("Unabled to start a background task for RealESRGAN: {}", err);
                error_dialog(&err);
                break;
            } else {
                self.processing = true;
            }
        }
    }

    fn children_status_checker() -> Subscription<Message> {
        struct ChildrenStatusWorker;

        iced::subscription::channel(
            std::any::TypeId::of::<ChildrenStatusWorker>(),
            100,
            |mut output| async move {
                let mut state = CheckerStatus::Starting;

                loop {
                    match &mut state {
                        CheckerStatus::Starting => {
                            let (sender, receiver) = mpsc::channel(8);

                            // If we fail to deliver even the Ready message, just crash
                            // and burn.
                            output.send(Message::CheckerReady(sender)).await.unwrap();

                            state = CheckerStatus::Ready(Vec::new(), receiver);
                        }
                        CheckerStatus::Ready(ref mut children, receiver) => {
                            use iced::futures::StreamExt;

                            let input = receiver.select_next_some().await;

                            match input {
                                CheckerTask::NewChild {
                                    input_path,
                                    output_path,
                                    upscale_ratio,
                                    gpu_id,
                                    model_path,
                                    model_name,
                                    tta_mode,
                                } => {
                                    let mut child = Cmd::new("./realesrgan-ncnn-vulkan-cli");
                                    let mut child = child
                                        .stderr(std::process::Stdio::piped())
                                        .arg("-i")
                                        .arg(input_path)
                                        .arg("-o")
                                        .arg(output_path)
                                        .arg("-s")
                                        .arg(upscale_ratio.to_string());

                                    if !gpu_id.is_empty() {
                                        child = child.arg("-g").arg(gpu_id);
                                    }

                                    if !model_path.is_empty() {
                                        child = child.arg("-m").arg(&model_path);
                                    }

                                    if !model_name.is_empty() {
                                        child = child.arg("-n").arg(&model_name);
                                    }

                                    if tta_mode {
                                        child = child.arg("-x");
                                    }

                                    match child.spawn() {
                                        Ok(c) => children.push(c),
                                        Err(e) => {
                                            // TODO: is unwrap() good here?
                                            output
                                                .send(Message::ChildUpdate(
                                                    CheckerResult::SpawnError(e.to_string()),
                                                ))
                                                .await
                                                .unwrap();
                                        }
                                    };
                                }
                                CheckerTask::Poll => {
                                    Self::check_children_status(children, &mut output).await;
                                }
                            }
                        }
                    }
                }
            },
        )
    }

    async fn check_children_status(children: &mut Vec<Child>, output: &mut mpsc::Sender<Message>) {
        if children.is_empty() {
            return;
        }

        let mut i = 0;

        while i < children.len() {
            let c = &mut children[i];

            let should_remove = match c.try_status() {
                Ok(None) => {
                    let pid = c.id();

                    if let Some(stderr) = c.stderr.as_mut().take() {
                        let mut reader = BufReader::new(stderr);
                        let mut log = String::new();
                        match reader.read_line(&mut log).await {
                            Ok(_) => {
                                // TODO: is unwrap() good here?
                                output
                                    .send(Message::ChildUpdate(CheckerResult::ChildLog(pid, log)))
                                    .await
                                    .unwrap();
                            }
                            Err(_) => (),
                        }
                    }
                    false
                }

                Ok(Some(status)) => {
                    // TODO: is unwrap() good here?
                    output
                        .send(Message::ChildUpdate(CheckerResult::ChildExited(
                            c.id(),
                            status,
                        )))
                        .await
                        .unwrap();
                    true
                }

                Err(e) => {
                    // TODO: is unwrap() good here?
                    output
                        .send(Message::ChildUpdate(CheckerResult::ChildErrored(
                            c.id(),
                            e.to_string(),
                        )))
                        .await
                        .unwrap();
                    true
                }
            };

            if should_remove {
                children.remove(i);
            } else {
                i += 1;
            }
        }

        if children.is_empty() {
            // TODO: is unwrap good here?
            output
                .send(Message::ChildUpdate(CheckerResult::Ended))
                .await
                .unwrap();
        }
    }

    fn apply_checker_updates(&mut self, result: CheckerResult) {
        use CheckerResult::*;

        let make_log = |buffer: &mut VecDeque<String>, log| {
            buffer.push_back(log);

            if buffer.len() >= 255 {
                buffer.pop_front();
            }
        };

        match result {
            Ended => {
                self.processing = false;
            }

            ChildLog(pid, raw_log) => {
                let log = format!("pid #{}: {}", pid, raw_log);
                make_log(&mut self.log, log);
            }

            ChildExited(pid, exit) => {
                if !exit.success() {
                    self.show_error_on_start_button(&format!(
                        "realesrgan returned {}",
                        exit.code().unwrap_or(-1)
                    ));
                } else {
                    make_log(&mut self.log, format!("pid #{}: complete!", pid));
                }
            }

            ChildErrored(pid, err) => {
                rfd::MessageDialog::new()
                    .set_title("Error")
                    .set_level(rfd::MessageLevel::Error)
                    .set_description(&format!(
                        "Unexpected error occured while running RealESRGAN: {}",
                        err
                    ))
                    .show();

                make_log(&mut self.log, format!("pid #{} ERROR: {}", pid, err));
            }

            SpawnError(err) => {
                rfd::MessageDialog::new()
                    .set_title("Error")
                    .set_level(rfd::MessageLevel::Error)
                    .set_description(&format!("Unable to spawn a realesrgan instance:\n{}", err))
                    .show();
            }
        }
    }
}

impl Application for RealEsrgan {
    type Executor = executor::Default;
    type Message = Message;
    type Theme = Theme;
    type Flags = ();

    fn new(_flags: ()) -> (Self, Command<Message>) {
        (
            Self {
                start_button_text: String::from("Click Here to Start"),
                filename_format: String::from("{name}-{scale}x"),
                ..Default::default()
            },
            Command::none(),
        )
    }

    fn title(&self) -> String {
        String::from("realesrgan-ncnn-vulkan-gui")
    }

    fn update(&mut self, message: Message) -> Command<Message> {
        match message {
            Message::AdvancedOptionsClicked(check) => self.advanced_options = check,
            Message::AskPath {
                path_type: PathType::Input,
            } => {
                let dialog = rfd::FileDialog::new()
                    .add_filter("Supported images", &["png", "jpg", "jpeg", "webp"])
                    .add_filter("PNG images", &["png"])
                    .add_filter("JPEG images", &["jpg", "jpeg"])
                    .add_filter("WebP images", &["webp"])
                    .set_title("Input files")
                    .pick_files();

                if let Some(files) = dialog {
                    if files.len() > 0 {
                        let path = files[0].to_string_lossy().to_string();
                        self.input = path;
                    }
                    self.state.selected_files =
                        files.into_iter().map(|p| p.into_os_string()).collect();
                }
            }
            Message::AskPath {
                path_type: PathType::Output,
            } => {
                let dialog = rfd::FileDialog::new().pick_folder();

                if let Some(dir) = dialog {
                    self.state.output_dir = dir.into();
                    self.output = self.state.output_dir.to_string_lossy().to_string();
                }
            }
            Message::CheckerReady(sender) => self.checker = Some(sender),
            Message::ChildUpdate(result) => self.apply_checker_updates(result),
            Message::GpuIdChanged(id) => self.gpu_id = id,
            Message::ModelNameChanged(name) => self.model_name = name,
            Message::ModelPathChanged(path) => self.model_path = path,
            Message::OutputFormatChanged(format) => self.format = format,
            Message::OutputNameChanged(name) => self.filename_format = name,
            Message::PathChanged { path_type, path } => match path_type {
                PathType::Input => {
                    self.state.selected_files.clear();
                    self.input = path
                }
                PathType::Output => {
                    self.state.output_dir.clear();
                    self.output = path
                }
            },
            Message::StartClicked => self.start(),
            Message::SwitchPage(page) => self.current_page = page,
            Message::Tick => {
                let _ = self.checker.as_mut().unwrap().try_send(CheckerTask::Poll);
            }
            Message::TTAModeClicked(check) => self.tta_mode = check,
            Message::UpscaleRatioSelected(ratio) => self.upscale_ratio = ratio,
        };

        Command::none()
    }

    fn view(&self) -> Element<Message> {
        let textbox = |label, text_ref, path_type| {
            row![
                text(label).size(20).width(100),
                text_input("Path", text_ref)
                    .id(text_input::Id::new(
                        ["text-in", "text-out"][path_type as usize]
                    ))
                    .on_input(move |path| Message::PathChanged { path_type, path })
                    .size(20),
                button(" ... ").on_press(Message::AskPath { path_type }),
            ]
            .align_items(Alignment::Center)
            .spacing(8)
            .padding(8)
        };

        let textboxes = column![
            textbox("Input path: ", &self.input, PathType::Input),
            textbox("Output path: ", &self.output, PathType::Output),
        ]
        .padding(8)
        .align_items(Alignment::Start);

        let mut start_button = button(self.start_button_text.as_ref()).width(Length::Fill);

        if !self.processing {
            start_button = start_button.on_press(Message::StartClicked)
        };

        let start = row![Space::with_width(16), start_button, Space::with_width(16),];

        let menubar = row![
            button("Processing")
                .on_press(Message::SwitchPage(Page::Processing))
                .width(120)
                .style(match self.current_page {
                    Page::Processing => theme::Button::Primary,
                    Page::Output => theme::Button::Secondary,
                    Page::Log => theme::Button::Secondary,
                }),
            button("Output")
                .on_press(Message::SwitchPage(Page::Output))
                .width(120)
                .style(match self.current_page {
                    Page::Processing => theme::Button::Secondary,
                    Page::Output => theme::Button::Primary,
                    Page::Log => theme::Button::Secondary,
                }),
            button("Log")
                .on_press(Message::SwitchPage(Page::Log))
                .width(120)
                .style(match self.current_page {
                    Page::Processing => theme::Button::Secondary,
                    Page::Output => theme::Button::Secondary,
                    Page::Log => theme::Button::Primary,
                }),
        ]
        .align_items(Alignment::Center)
        .spacing(8)
        .padding(8);

        // This is made a macro to workaround callback type checking woes
        macro_rules! textbox {
            (INTERNAL $label:expr, $text_ref:expr, $callback:expr, $advanced:expr) => {{
                let mut text = text($label)
                    .size(20)
                    .width(160)
                    .style(Color::from([0.5, 0.5, 0.5]));
                let mut textbox = text_input("", $text_ref).size(20);

                // Do not set on_input without advanced_options to disable
                // the textboxes
                if !$advanced || self.advanced_options {
                    text = text.style(theme::Text::Default);
                    textbox = textbox.on_input($callback);
                }

                row![
                    text,
                    textbox,
                ]
                .align_items(Alignment::Center)
                .spacing(8)
                .padding(8)
            }};

            (advanced $label:expr, $text_ref:expr, $callback:expr) => {
                textbox!(INTERNAL $label, $text_ref, $callback, true)
            };

            ($label:expr, $text_ref:expr, $callback:expr) => {
                textbox!(INTERNAL $label, $text_ref, $callback, false)
            };
        }

        let menu = match self.current_page {
            Page::Processing => {
                let option = |label, value| {
                    radio(label, value, Some(self.upscale_ratio), |val| {
                        Message::UpscaleRatioSelected(val)
                    })
                    .size(20)
                };

                let upscale_ratio = row![
                    text("Upscale ratio ").size(20).width(120),
                    option("1x", UpscaleRatio::One),
                    option("2x", UpscaleRatio::Two),
                    option("3x", UpscaleRatio::Three),
                    option("4x", UpscaleRatio::Four),
                ]
                .padding(16)
                .spacing(32);

                column![
                    upscale_ratio,
                    column![
                        checkbox(
                            "Enable TTA mode (performance intensive)",
                            self.tta_mode,
                            |b| Message::TTAModeClicked(b)
                        ),
                        checkbox(
                            "Advanced options",
                            self.advanced_options,
                            |b| Message::AdvancedOptionsClicked(b)
                        ),
                    ]
                        .spacing(12)
                        .padding(12),
                    textbox!(advanced "GPU ID", &self.gpu_id, |id| Message::GpuIdChanged(id)),
                    textbox!(advanced "Path to Model", &self.model_path, |path| Message::ModelPathChanged(path)),
                    textbox!(advanced "RealESRGAN Model", &self.model_name, |name| Message::ModelNameChanged(name)),
                ]
                .align_items(Alignment::Start)
                .padding(8)
                .spacing(8)
            }

            Page::Output => {
                let option = |label, value| {
                    radio(label, value, Some(self.format), |val| {
                        Message::OutputFormatChanged(val)
                    })
                    .size(20)
                };

                let format_radio = row![
                    text("Output Format").size(20).width(160),
                    option("PNG", Format::Png),
                    option("JPG", Format::Jpg),
                    option("WebP", Format::Webp),
                ]
                .padding(16)
                .spacing(16);

                column![
                    format_radio,
                    textbox!("Output Name", &self.filename_format, |name| {
                        Message::OutputNameChanged(name)
                    })
                    .padding(16),
                    text(concat!(
                        "{name}, {scale}, {model} will be replaced with specific values.\n",
                        "An extension will be automatically appended to the filename."
                    ))
                    .size(16)
                ]
                .align_items(Alignment::Start)
                .padding(16)
                .spacing(16)
            }

            Page::Log => {
                let mut log_screen = column![];

                for log in self.log.iter() {
                    log_screen = log_screen.push(text(log));
                }

                let scrollable_log = scrollable(row![
                    Space::with_width(16),
                    log_screen.width(Length::Fill),
                    Space::with_width(16),
                ]);

                column![scrollable_log]
            }
        };

        column![textboxes, start, menubar, menu]
            .align_items(Alignment::Center)
            .into()
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        Subscription::batch([
            iced::time::every(Duration::from_millis(500)).map(|_| Message::Tick),
            Self::children_status_checker(),
        ])
    }
}
