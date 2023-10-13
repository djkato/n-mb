#![feature(exit_status_error)]

use anyhow::Context;
use clap::{arg, command, value_parser, ValueEnum};
use encoder::{FFMPEGCommand, MediaType};
use std::{path::PathBuf, process::Stdio, sync::Arc};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    sync::Mutex,
};
use ui::display;

use crate::encoder::EncodingStatus;
mod encoder;
mod ui;

#[derive(Debug, Clone)]
pub enum VideoCodec {
    WEBM,
    HEVC,
}

impl std::fmt::Display for VideoCodec {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::WEBM => write!(f, "WEBM"),
            Self::HEVC => write!(f, "HEVC"),
        }
    }
}

impl VideoCodec {
    pub fn from_string(string: &str) -> Option<Self> {
        match string.to_lowercase().as_str() {
            "webm" => Some(Self::WEBM),
            "hevc" => Some(Self::HEVC),
            _ => None,
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = command!()
        .about("Simple program to parse files to the most efficient formats within a set size")
        .arg(
            arg!(-s --size <NUMBER> "Target megabyte size. If not set, default of 25mb (Discords free limit)")
            .required(false)
            .default_value("25")
            .value_parser(value_parser!(u16))
            )
        .arg(
            arg!(-c --codec <CODEC> "Choose video codec between `HEVC` (H.265) and `WEBM` (vp9).")
            .required(false)
            .default_value("WEBM")
            )
        .arg(
            arg!(-f --files <FILES> "Comma separated files to convert. EG: -f=<FILE>,<FILE>")
            .required(true)
            .value_parser(value_parser!(PathBuf))
            .value_delimiter(',')
            .num_args(1..=std::usize::MAX)
        ).get_matches();
    let size = args
        .get_one::<u16>("size")
        .expect("Default value dissapeared from rate")
        * 8;
    let files = args
        .get_many::<PathBuf>("files")
        .context("No files specified")?
        .collect::<Vec<_>>();

    let binding = "webm".to_owned();
    let codec = args.get_one::<String>("codec").unwrap_or(&binding);
    let codec = VideoCodec::from_string(codec).unwrap_or(VideoCodec::WEBM);

    let commands: Arc<Mutex<Vec<FFMPEGCommand>>> = Arc::new(Mutex::new(vec![]));
    {
        let mut commands_mut = commands.try_lock().unwrap();
        for file in files {
            let mut command: FFMPEGCommand;
            let extension = file
                .extension()
                .context("File doesn't have extension - is folder or is invalid file")?;

            match extension
                .to_str()
                .expect("Somehow Extension contains charcters we can't decode lol")
                .to_lowercase()
                .as_str()
            {
                "webm" | "mp4" | "mov" | "avi" | "mpeg" | "mkv" => {
                    command =
                        FFMPEGCommand::new(MediaType::Video, file, size.clone(), codec.clone())
                            .await?;
                }
                "mp3" | "wav" | "ogg" | "opus" | "flac" | "aiff" => {
                    command =
                        FFMPEGCommand::new(MediaType::Audio, file, size.clone(), codec.clone())
                            .await?;
                }
                "jpg" | "png" | "webp" | "exr" | "jpeg" | "tiff" | "bpm" | "raw" | "tif" => {
                    command =
                        FFMPEGCommand::new(MediaType::Image, file, size.clone(), codec.clone())
                            .await?;
                }
                "gif" => {
                    command = FFMPEGCommand::new(
                        MediaType::AnimatedImage,
                        file,
                        size.clone(),
                        codec.clone(),
                    )
                    .await?;
                }
                _ => break,
            }
            dbg!(&command.command.0);

            command.command.0.stdout(Stdio::piped());
            command.command.0.stderr(Stdio::null());
            command.command.0.stdin(Stdio::null());
            command.command.0.kill_on_drop(true);
            if command.media_type == MediaType::Video {
                let mut pass2 = command.command.1.unwrap();
                pass2.stdout(Stdio::piped());
                pass2.stderr(Stdio::null());
                pass2.stdin(Stdio::null());
                pass2.kill_on_drop(true);
                command.command.1 = Some(pass2)
            }

            command.exec_handle = Some(command.command.0.spawn()?);
            command.buff_reader = Some(
                BufReader::new(
                    command
                        .exec_handle
                        .as_mut()
                        .unwrap()
                        .stdout
                        .take()
                        .expect("encoder stdout missing - exited early or unavailable"),
                )
                .lines(),
            );
            commands_mut.push(command);
        }
    }
    let mut command_spawns = vec![];
    let mut buff_readers = vec![];

    let ui = tokio::spawn(display(commands.clone()));

    {
        for (i, command) in commands.lock().await.iter_mut().enumerate() {
            buff_readers.push((i, command.buff_reader.take().unwrap()));
        }
    }
    for mut buff_reader in buff_readers.into_iter() {
        use std::time::Duration;
        use tokio::time::interval;
        let commands_ref = commands.clone();
        let mut intv = interval(Duration::from_millis(50));

        command_spawns.push(tokio::spawn(async move {
            intv.tick().await;

            'line: while let Ok(Some(line)) = buff_reader.1.next_line().await {
                if let Some(time_start) = line.find("out_time=") {
                    let time: Vec<String> = line[time_start + 10..]
                        .split(":")
                        .map(|s| s.to_owned())
                        .collect();

                    let mut parsed_time = vec![];

                    for part in time {
                        if let Ok(number) = part.parse::<f32>() {
                            parsed_time.push(number)
                        } else {
                            break 'line;
                        }
                    }
                    let time = parsed_time[0] * 3600. + parsed_time[1] * 60. + parsed_time[2];

                    let mut command = commands_ref.lock().await;
                    let command = command.get_mut(buff_reader.0).unwrap();

                    command.status = EncodingStatus::InProgress;
                    command.progressed_time = time;
                }
                if let Some(progress_i) = line.find("progress=") {
                    let mut command = commands_ref.lock().await;
                    let command = command.get_mut(buff_reader.0).unwrap();

                    match &line[progress_i + 9..] {
                        "end" => match command.media_type {
                            //Executes 2nd pass
                            MediaType::Video => match command.passed_pass_1 {
                                true => command.status = EncodingStatus::Finished,
                                false => {
                                    command.exec_handle =
                                        Some(command.command.1.as_mut().unwrap().spawn().unwrap());
                                    buff_reader = (
                                    buff_reader.0,
                                    BufReader::new(
                                        command.exec_handle.as_mut().unwrap().stdout.take().expect(
                                            "encoder stdout missing - exited early or unavailable",
                                        ),
                                    )
                                    .lines(),
                                );
                                    command.passed_pass_1 = true;
                                }
                            },
                            _ => command.status = EncodingStatus::Finished,
                        },
                        "continue" => command.status = EncodingStatus::InProgress,
                        _ => (),
                    };
                }
            }
        }));
    }
    for spawn in command_spawns {
        spawn.await?;
    }
    ui.await?;
    Ok(())
}
