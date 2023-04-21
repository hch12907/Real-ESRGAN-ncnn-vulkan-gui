use std::ffi::OsString;
use std::process::ExitStatus;

use async_std::io::{prelude::BufReadExt, BufReader};
use async_std::process::{Child, Command};

use iced::futures::{channel::mpsc, SinkExt};
use iced::Subscription;

use crate::Message;

pub struct ChildrenStatusChecker;

#[derive(Debug)]
enum CheckerStatus {
    Starting,
    Ready(Vec<Child>, mpsc::Receiver<CheckerTask>),
}

#[derive(Clone, Debug)]
pub enum CheckerResult {
    Ended,
    ChildLog(u32, String),        // (pid, message)
    ChildExited(u32, ExitStatus), // (pid, error_code)
    ChildErrored(u32, String),    // (pid, error)
    SpawnError(String),
}

pub enum CheckerTask {
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

impl ChildrenStatusChecker {
    pub fn children_status_checker() -> Subscription<Message> {
        iced::subscription::channel(
            std::any::TypeId::of::<Self>(),
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
                                    let mut child = Command::new("./realesrgan-ncnn-vulkan-cli");
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
}
