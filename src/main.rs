#![windows_subsystem = "windows"]

extern crate native_windows_derive as nwd;
extern crate native_windows_gui as nwg;

use std::cell::RefCell;
use std::ffi::OsString;
use std::os::windows::prelude::{OsStrExt, OsStringExt};
use std::path::PathBuf;
use std::process::{Child, Command};

use nwd::NwgUi;
use nwg::{
    AnimationTimer, CheckBox, CheckBoxState, EventData, Font, MessageChoice, MessageIcons,
    MessageParams, NativeUi, RadioButton, Tab, TabsContainer, TextInput,
};

const WHITE: Option<[u8; 3]> = Some([255, 255, 255]);

#[derive(Clone, Debug, Default, PartialEq)]
enum Format {
    #[default]
    Png,
    Jpg,
    Webp,
}

#[derive(Default, NwgUi)]
pub struct RealEsrganApp {
    #[nwg_control(size: (700, 430), title: "realesrgan-ncnn-vulkan")]
    #[nwg_events(
        OnInit: [RealEsrganApp::on_init],
        OnMinMaxInfo: [RealEsrganApp::on_minmax(SELF, EVT_DATA)],
        OnWindowClose: [RealEsrganApp::on_quit]
    )]
    window: nwg::Window,

    #[nwg_layout(parent: window, spacing: 3)]
    grid: nwg::GridLayout,

    #[nwg_control(text: "Input path:")]
    #[nwg_layout_item(layout: grid, row: 0, col: 0, col_span: 2)]
    input_label: nwg::Label,

    #[nwg_control(text: "")]
    #[nwg_layout_item(layout: grid, row: 0, col: 2, col_span: 12)]
    input_path: nwg::TextInput,

    #[nwg_control(text: "...")]
    #[nwg_events(OnButtonClick: [RealEsrganApp::select_input_file])]
    #[nwg_layout_item(layout: grid, row: 0, col: 14)]
    input_button: nwg::Button,

    #[nwg_control(text: "Output path:")]
    #[nwg_layout_item(layout: grid, row: 1, col: 0, col_span: 2)]
    output_label: nwg::Label,

    #[nwg_control(text: "")]
    #[nwg_layout_item(layout: grid, row: 1, col: 2, col_span: 12)]
    output_path: nwg::TextInput,

    #[nwg_control(text: "...")]
    #[nwg_layout_item(layout: grid, row: 1, col: 14)]
    #[nwg_events(OnButtonClick: [RealEsrganApp::select_output_file])]
    output_button: nwg::Button,

    #[nwg_control(text: "Start")]
    #[nwg_layout_item(layout: grid, col: 0, row: 2, row_span: 1, col_span: 15)]
    #[nwg_events( OnButtonClick: [RealEsrganApp::start_clicked] )]
    start_button: nwg::Button,

    // `tabs` begin here
    #[nwg_control]
    #[nwg_layout_item(layout: grid, col: 0, row: 3, row_span: 9, col_span: 15)]
    tabs: TabsContainer,

    // `tabs::processing_tab` begins here
    #[nwg_control(text: "Processing")]
    processing_tab: Tab,

    #[nwg_layout(parent: processing_tab, spacing: 2, margin: [1, 5, 1, 5])]
    tab_grid: nwg::GridLayout,

    #[nwg_control(text: "Upscale Ratio", background_color: WHITE)]
    #[nwg_layout_item(layout: tab_grid, col: 0, row: 0, col_span: 2)]
    upscale_label: nwg::Label,

    #[nwg_control(
        text: "1x",
        background_color: WHITE,
        flags: "VISIBLE|GROUP", 
        check_state: RadioButtonState::Checked
    )]
    #[nwg_layout_item(layout: tab_grid, col: 2, row: 0, col_span: 1)]
    #[nwg_events(OnButtonClick: [RealEsrganApp::upscale_clicked(SELF, CTRL)])]
    upscale_level1: RadioButton,

    #[nwg_control(text: "2x", background_color: WHITE)]
    #[nwg_layout_item(layout: tab_grid, col: 3, row: 0, col_span: 1)]
    #[nwg_events(OnButtonClick: [RealEsrganApp::upscale_clicked(SELF, CTRL)])]
    upscale_level2: RadioButton,

    #[nwg_control(text: "3x", background_color: WHITE)]
    #[nwg_layout_item(layout: tab_grid, col: 4, row: 0, col_span: 1)]
    #[nwg_events(OnButtonClick: [RealEsrganApp::upscale_clicked(SELF, CTRL)])]
    upscale_level3: RadioButton,

    #[nwg_control(text: "4x", background_color: WHITE)]
    #[nwg_layout_item(layout: tab_grid, col: 5, row: 0, col_span: 1)]
    #[nwg_events(OnButtonClick: [RealEsrganApp::upscale_clicked(SELF, CTRL)])]
    upscale_level4: RadioButton,

    #[nwg_control(text: "Enable TTA Mode (performance intensive)", background_color: WHITE)]
    #[nwg_layout_item(layout: tab_grid, col: 0, row: 1, col_span: 5)]
    #[nwg_events(OnButtonClick: [RealEsrganApp::tta_mode_clicked])]
    tta_mode: CheckBox,

    #[nwg_control(text: "Advanced Options", background_color: WHITE)]
    #[nwg_layout_item(layout: tab_grid, col: 0, row: 2, col_span: 5)]
    #[nwg_events(OnButtonClick: [RealEsrganApp::advanced_options_clicked])]
    advanced_options: CheckBox,

    #[nwg_control(text: "GPU ID", background_color: WHITE)]
    #[nwg_layout_item(layout: tab_grid, col: 0, row: 3, col_span: 2)]
    gpu_id_label: nwg::Label,

    #[nwg_control(text: "auto", background_color: WHITE, readonly: true)]
    #[nwg_layout_item(layout: tab_grid, col: 2, row: 3, col_span: 7)]
    #[nwg_events(OnTextInput: [RealEsrganApp::gpu_id_changed])]
    gpu_id: TextInput,

    #[nwg_control(text: "Path to Model", background_color: WHITE)]
    #[nwg_layout_item(layout: tab_grid, col: 0, row: 4, col_span: 2)]
    model_path_label: nwg::Label,

    #[nwg_control(text: "", background_color: WHITE, readonly: true)]
    #[nwg_layout_item(layout: tab_grid, col: 2, row: 4, col_span: 7)]
    #[nwg_events(OnTextInput: [RealEsrganApp::model_path_changed])]
    model_path: TextInput,

    #[nwg_control(text: "RealESRGAN Model", background_color: WHITE)]
    #[nwg_layout_item(layout: tab_grid, col: 0, row: 5, col_span: 2)]
    model_label: nwg::Label,

    #[nwg_control(text: "realesrgan-x4plus-anime", background_color: WHITE, readonly: true)]
    #[nwg_layout_item(layout: tab_grid, col: 2, row: 5, col_span: 7)]
    #[nwg_events(OnTextInput: [RealEsrganApp::model_name_changed])]
    model_name: TextInput,

    // `tabs::processing_tab` ends here
    // `tabs::output_tab` begins here
    #[nwg_control(parent: tabs, text: "Output")]
    output_tab: Tab,

    #[nwg_control(text: "Output Format", background_color: WHITE)]
    #[nwg_layout_item(layout: tab_grid, col: 0, row: 0, col_span: 2)]
    format_label: nwg::Label,

    #[nwg_control(
        text: "PNG",
        background_color: WHITE,
        flags: "VISIBLE|GROUP", 
        check_state: RadioButtonState::Checked
    )]
    #[nwg_layout_item(layout: tab_grid, col: 2, row: 0, col_span: 1)]
    #[nwg_events(OnButtonClick: [RealEsrganApp::format_clicked(SELF, CTRL)])]
    format_png: RadioButton,

    #[nwg_control(text: "JPG", background_color: WHITE)]
    #[nwg_layout_item(layout: tab_grid, col: 3, row: 0, col_span: 1)]
    #[nwg_events(OnButtonClick: [RealEsrganApp::format_clicked(SELF, CTRL)])]
    format_jpg: RadioButton,

    #[nwg_control(text: "WebP", background_color: WHITE)]
    #[nwg_layout_item(layout: tab_grid, col: 4, row: 0, col_span: 1)]
    #[nwg_events(OnButtonClick: [RealEsrganApp::format_clicked(SELF, CTRL)])]
    format_webp: RadioButton,

    #[nwg_control(text: "Output Name", background_color: WHITE)]
    #[nwg_layout_item(layout: tab_grid, col: 0, row: 1, col_span: 2)]
    filename_label: nwg::Label,

    #[nwg_control(text: "{name}_{scale}x", background_color: WHITE)]
    #[nwg_layout_item(layout: tab_grid, col: 2, row: 1, col_span: 7)]
    #[nwg_events(OnTextInput: [RealEsrganApp::filename_changed])]
    filename_format: TextInput,

    #[nwg_control(text: "{name}, {scale}, {model} will be replaced with specific values.\nAn extension will be automatically appended to the filename.", background_color: WHITE)]
    #[nwg_layout_item(layout: tab_grid, col: 0, row: 2, col_span: 8, row_span: 2)]
    filename_advice_label: nwg::Label,

    // `tabs::output_tab` ends here
    #[nwg_resource(
        title: "Open File",
        action: nwg::FileDialogAction::Open,
        multiselect: true,
        filters: "PNG(*.png)|JPEG(*.jpg;*.jpeg)|WebP(*.webp)|>Supported image files(*.png;*.jpg;*.jpeg;*.webp)"
    )]
    open_file_dialog: nwg::FileDialog,

    #[nwg_resource(
        title: "Save File",
        action: nwg::FileDialogAction::OpenDirectory
    )]
    save_file_dialog: nwg::FileDialog,

    #[nwg_control(parent: window, interval: std::time::Duration::from_millis(100))]
    #[nwg_events(OnTimerTick: [RealEsrganApp::timer_ticked])]
    timer: AnimationTimer,

    #[nwg_resource(family: "Segoe UI", size: 16)]
    advice_font: Font,

    state: RefCell<RealEsrganState>,
}

pub struct RealEsrganState {
    selected_files: Vec<OsString>,
    output_dir: OsString,
    scale_level: i32,
    tta_mode: bool,
    format: Format,
    gpu_id: String,
    model_name: String,
    model_path: String,
    filename_format: String,
    children: Vec<Child>,
}

impl Default for RealEsrganState {
    fn default() -> Self {
        Self {
            selected_files: Vec::new(),
            output_dir: OsString::new(),
            scale_level: 1,
            tta_mode: false,
            format: Format::Png,
            gpu_id: String::new(),
            model_name: String::from("realesrgan-x4plus-anime"),
            model_path: String::new(),
            filename_format: String::from("{name}_{scale}x"),
            children: Vec::new(),
        }
    }
}

impl RealEsrganState {
    fn set_scale_level(&mut self, level: i32) {
        self.scale_level = level;
    }
}

impl RealEsrganApp {
    fn on_init(&self) {
        self.filename_advice_label.set_font(Some(&self.advice_font));
    }

    fn on_minmax(&self, data: &EventData) {
        data.on_min_max().set_min_size(700, 450);
    }

    fn on_quit(&self) {
        nwg::stop_thread_dispatch();
    }

    fn select_input_file(&self) {
        if self.open_file_dialog.run(Some(&self.window)) {
            self.input_path.set_text("");
            if let Ok(paths) = self.open_file_dialog.get_selected_items() {
                let viewable_paths = paths
                    .iter()
                    .take(10)
                    .map(|p| p.to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join(";");

                self.state.borrow_mut().selected_files = paths.clone();
                self.input_path.set_text(&viewable_paths);
            }
        }
    }

    fn format_clicked(&self, control: &RadioButton) {
        let format = match control.text().as_ref() {
            "PNG" => Format::Png,
            "JPG" => Format::Jpg,
            "WebP" => Format::Webp,
            _ => unreachable!("invalid format detected"),
        };

        self.state.borrow_mut().format = format;
    }

    fn upscale_clicked(&self, control: &RadioButton) {
        let text = control.text();
        let level = text.trim_end_matches('x').parse::<i32>().unwrap();
        self.state.borrow_mut().set_scale_level(level);
    }

    fn tta_mode_clicked(&self) {
        self.state.borrow_mut().tta_mode = self.tta_mode.check_state() == CheckBoxState::Checked;
    }

    fn advanced_options_clicked(&self) {
        let advanced = self.advanced_options.check_state() == CheckBoxState::Checked;

        self.gpu_id.set_readonly(!advanced);
        self.model_name.set_readonly(!advanced);
        self.model_path.set_readonly(!advanced);
    }

    fn select_output_file(&self) {
        if self.save_file_dialog.run(Some(&self.window)) {
            self.output_path.set_text("");
            if let Ok(path) = self.save_file_dialog.get_selected_item() {
                self.state.borrow_mut().output_dir = path.clone();
                self.output_path.set_text(path.to_string_lossy().as_ref());
            }
        }
    }

    fn model_path_changed(&self) {
        self.state.borrow_mut().model_path = self.model_path.text();
    }

    fn gpu_id_changed(&self) {
        self.state.borrow_mut().gpu_id = self.gpu_id.text();
    }

    fn model_name_changed(&self) {
        self.state.borrow_mut().model_name = self.model_name.text();
    }

    fn filename_changed(&self) {
        self.state.borrow_mut().filename_format = self.filename_format.text();
    }

    fn timer_ticked(&self) {
        // It is possible for the timer tick event to fire while an error
        // message is being shown in the following while loop, which leads to
        // a panic 
        let state = &mut *match self.state.try_borrow_mut() {
            Ok(x) => x,
            Err(_) => return,
        };

        let mut i = 0;
        while i < state.children.len() {
            let c = &mut state.children[i];

            let should_remove = match c.try_wait() {
                Ok(None) => false,
                Ok(Some(status)) => {
                    if !status.success() {
                        self.start_button.set_text("Processing... (error occured!)");
                        true
                    } else {
                        true
                    }
                }
                Err(e) => {
                    nwg::modal_error_message(
                        &self.window,
                        "Error",
                        &format!("Unexpected error occured while running RealESRGAN: {}", e),
                    );
                    true
                }
            };

            if should_remove {
                state.children.remove(i);
            } else {
                i += 1;
            }
        }

        if state.children.is_empty() {
            self.timer.stop();
            if self.start_button.text().contains("error") {
                self.start_button.set_text("Start (error occured!)");
            } else {
                self.start_button.set_text("Start")
            }
            self.start_button.set_enabled(true);
        }
    }

    fn start_clicked(&self) {
        let mut state = self.state.borrow_mut();
        let mut children = Vec::new();

        if state.selected_files.is_empty() {
            return;
        }

        if state.scale_level == 1 {
            nwg::modal_info_message(
                &self.window,
                "Error",
                "An upscale ratio greater than 1 is not specified.",
            );
            return;
        }

        if state.scale_level < 4 && state.model_name.contains("realesrgan-x4plus") {
            let params = MessageParams {
                title: "Unsupported upscale ratio",
                content: concat!(
                    "The upscale ratio is possibly incompatible with the model.\n",
                    "The output may possibly be distorted.\n\n",
                    "Do you wish to continue?"
                ),
                buttons: nwg::MessageButtons::YesNo,
                icons: MessageIcons::Question,
            };
            let choice = nwg::modal_message(&self.window, &params);

            if choice == MessageChoice::No {
                return;
            }
        }

        let output_dir = if !state.output_dir.is_empty() {
            state.output_dir.clone()
        } else {
            let params = MessageParams {
                title: "Output path selection",
                content: concat!(
                    "The output path is left unspecified.\n\n",
                    "By default, this means the directory containing the application will be used.\n\n",
                    "Do you want to change it to the input directory?"
                ),
                buttons: nwg::MessageButtons::YesNoCancel,
                icons: MessageIcons::Question,
            };
            let choice = nwg::modal_message(&self.window, &params);

            if choice == MessageChoice::Yes {
                let mut path = PathBuf::from(&state.selected_files[0]);
                if path.is_file() {
                    path.pop();
                }
                self.output_path
                    .set_text(&path.to_string_lossy().trim_start_matches("\\\\?\\"));
                path.into_os_string()
            } else if choice == MessageChoice::No {
                let path = PathBuf::from(".").canonicalize().unwrap().into_os_string();
                state.output_dir = path.clone();
                self.output_path
                    .set_text(&path.to_string_lossy().trim_start_matches("\\\\?\\"));
                path
            } else {
                return;
            }
        };

        for f in state.selected_files.iter() {
            let mut input = PathBuf::from(f);
            let mut output = PathBuf::from(&output_dir);
            let output_ext = match state.format {
                Format::Png => "png",
                Format::Jpg => "jpg",
                Format::Webp => "webp",
            };

            input.set_extension("");

            output.push(match input.file_name() {
                Some(x) => {
                    let template = state
                        .filename_format
                        .replace("{scale}", &format!("{}", state.scale_level))
                        .replace("{model}", &state.model_name);

                    let name_start = match template.find("{name}") {
                        Some(x) => x,
                        None => {
                            nwg::error_message(
                                "Error",
                                "Output filename must contain a {name} section!",
                            );
                            return;
                        }
                    };

                    let name_end = name_start + "{name}".len();

                    let encoded = template
                        .encode_utf16()
                        .take(name_start)
                        .chain(OsStrExt::encode_wide(x))
                        .chain(template.encode_utf16().skip(name_end))
                        .collect::<Vec<_>>();

                    OsString::from_wide(&encoded)
                }
                None => {
                    nwg::error_message(
                        "Error",
                        &format!(
                            "The following input file has an invalid path: {}",
                            f.to_string_lossy()
                        ),
                    );
                    return;
                }
            });

            output.set_extension(output_ext);

            let mut realesrgan_command = Command::new("realesrgan-ncnn-vulkan-cli");
            let mut realesrgan = realesrgan_command
                .arg("-i")
                .arg(f)
                .arg("-o")
                .arg(output)
                .arg("-s")
                .arg(state.scale_level.to_string());

            if !state.gpu_id.is_empty() {
                realesrgan = realesrgan.arg("-g").arg(&state.gpu_id);
            }

            if !state.model_path.is_empty() {
                realesrgan = realesrgan.arg("-m").arg(&state.model_path);
            }

            if !state.model_name.is_empty() {
                realesrgan = realesrgan.arg("-n").arg(&state.model_name);
            }

            if state.tta_mode {
                realesrgan = realesrgan.arg("-x");
            }

            let child = match realesrgan.spawn() {
                Ok(x) => x,
                Err(e) => {
                    nwg::error_message(
                        "Error",
                        &format!("Unable to spawn a realesrgan instance:\n{:?}", e),
                    );
                    return;
                }
            };

            children.push(child);
        }

        state.children = children;

        drop(state);

        self.start_button.set_text("Processing...");
        self.start_button.set_enabled(false);
        self.timer.start();
    }
}

fn main() {
    nwg::init().expect("Failed to init Native Windows GUI");
    nwg::Font::set_global_family("Segoe UI").expect("Failed to set default font");
    let _app = RealEsrganApp::build_ui(Default::default()).expect("Failed to build UI");
    nwg::dispatch_thread_events();
}
