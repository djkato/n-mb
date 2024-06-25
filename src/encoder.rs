use anyhow::{bail, Context};
use pbr::{Pipe, ProgressBar};
use std::{path::PathBuf, process::Stdio};
use tokio::{
    io::{BufReader, Lines},
    process::{Child, ChildStdout, Command},
};

use crate::VideoCodec;
const MAX_OPUS_BITRATE: f32 = 256.; //kbits
const MIN_OPUS_BITRATE: f32 = 50.; //kbits

pub struct FFMPEGCommand {
    pub file_name: String,
    pub command: (Command, Option<Command>),
    pub target_size: u16,
    pub resolution: Option<(u32, u32)>,
    pub duration: Option<f32>,
    pub media_type: MediaType,
    pub exec_handle: Option<Child>,
    pub buff_reader: Option<Lines<BufReader<ChildStdout>>>,
    pub status: EncodingStatus,
    pub passed_pass_1: bool,
    pub progressed_time: f32,
    pub progress_bar: Option<ProgressBar<Pipe>>,
}

struct MediaData {
    resolution: Option<(u16, u16)>,
    duration: f32,
    old_kbit_rate: Option<u32>,
}

impl FFMPEGCommand {
    pub async fn new(
        media_type: MediaType,
        path: &PathBuf,
        size: u16,
        codec: VideoCodec,
    ) -> anyhow::Result<Self> {
        match media_type {
            MediaType::Video => Self::create_video(path, size, codec).await,
            MediaType::Audio => Self::create_audio(path, size).await,
            MediaType::Image => Self::create_image(path, size),
            MediaType::AnimatedImage => Self::create_animated_image(path),
        }
    }

    async fn create_audio(path: &PathBuf, size: u16) -> anyhow::Result<Self> {
        let ffprobe_out = parse_ffprobe(path).await?;
        let duration = ffprobe_out.duration;
        let max_kbit_rate = match ffprobe_out.old_kbit_rate {
            None => MAX_OPUS_BITRATE,
            Some(r) => {
                if (r as f32) < MAX_OPUS_BITRATE {
                    r as f32
                } else {
                    MAX_OPUS_BITRATE
                }
            }
        };

        let bitrate = (size as f32 * 1000. / duration) * 0.85;
        let bitrate = bitrate.clamp(MIN_OPUS_BITRATE, max_kbit_rate) as u16;
        /*
        println!(
            "{} * {} ~= {} (actually is {})",
            duration,
            bitrate,
            size * 1000,
            duration * bitrate as f32
        );
        */
        let mut new_path = path.clone();
        new_path.set_extension("ogg");

        let mut command = Command::new("ffmpeg");
        command.args(["-progress", "-", "-nostats", "-stats_period", "50ms"]);
        command.args([
            "-y",
            "-i",
            path.as_os_str()
                .to_str()
                .expect("Path dissapeared on unwrap"),
            "-c:a",
            "libopus",
            "-b:a",
            format!("{}k", bitrate).as_str(),
            new_path
                .as_os_str()
                .to_str()
                .expect("Path dissapeared on unwrap"),
        ]);
        Ok(FFMPEGCommand {
            file_name: path.file_name().unwrap().to_str().unwrap().to_owned(),
            resolution: None,
            duration: Some(duration),
            command: (command, None),
            media_type: MediaType::Audio,
            target_size: size,
            status: EncodingStatus::NotStarted,
            exec_handle: None,
            buff_reader: None,
            progress_bar: None,
            passed_pass_1: false,
            progressed_time: 0.,
        })
    }

    async fn create_video(path: &PathBuf, size: u16, codec: VideoCodec) -> anyhow::Result<Self> {
        let ffprobe_out = parse_ffprobe(path).await?;

        let duration = ffprobe_out.duration;
        let resolution = ffprobe_out.resolution.context("Missing resolution")?;

        let mut overflown_audio_bitrate = None;
        let mut audio_bitrate = size as f32 * 180. / duration;
        let mut video_bitrate = size as f32 * 780. / duration;

        if audio_bitrate < MIN_OPUS_BITRATE {
            overflown_audio_bitrate = Some(audio_bitrate - MIN_OPUS_BITRATE);
            audio_bitrate = MIN_OPUS_BITRATE;
        }
        if audio_bitrate > MAX_OPUS_BITRATE {
            overflown_audio_bitrate = Some(audio_bitrate - MAX_OPUS_BITRATE);
            audio_bitrate = MAX_OPUS_BITRATE;
        }

        if let Some(overflow) = overflown_audio_bitrate {
            /*
            println!(
                "-b:v:{}\n-b:a:{} (ovw: {})\nsum:{}/{}",
                video_bitrate,
                audio_bitrate,
                overflow,
                video_bitrate + audio_bitrate,
                size
            );*/
            video_bitrate += overflow;
        }

        let mut height = resolution.1;
        if resolution.1 >= 1080 && duration > 150. {
            height = 1080
        }
        if resolution.1 >= 720 && duration > 600. {
            height = 720
        }
        if resolution.1 >= 480 && duration > 900. {
            height = 480
        }

        let old_path_str = path.as_os_str().to_str().context("missing or bad path")?;
        let mut new_path = path.clone();

        let scale_arg = format!("scale=-1:{height}");
        let bitrate_arg = format!("{}k", video_bitrate as u16);
        let minrate_arg = format!("{}k", (video_bitrate * 0.5) as u16);
        let maxrate_arg = format!("{}k", (video_bitrate * 1.45) as u16);
        let ba_arg = format!("{}k", audio_bitrate as u16);
        let mut passlogfile = path.clone();
        passlogfile.set_extension("");
        let mut command = Command::new("ffmpeg");
        let mut command2 = Command::new("ffmpeg");
        command.args(["-progress", "-", "-nostats", "-stats_period", "50ms"]);
        command2.args(["-progress", "-", "-nostats", "-stats_period", "50ms"]);
        let video_codec;
        let audio_codec;
        match codec {
            VideoCodec::WEBM => {
                video_codec = "libvpx-vp9";
                audio_codec = "libopus";
                new_path.set_extension("webm");
                new_path.set_file_name(
                    "minified_".to_owned() + new_path.file_name().unwrap().to_str().unwrap(),
                )
            }
            VideoCodec::HEVC => {
                video_codec = "libx265";
                audio_codec = "aac";
                new_path.set_extension("mp4");
                new_path.set_file_name(
                    "minified_".to_owned() + new_path.file_name().unwrap().to_str().unwrap(),
                )
            }
        };
        /*
        println!(
            "{} * ({}+{}) ~= {} (actually is {})",
            duration,
            video_bitrate,
            audio_bitrate,
            size,
            (duration * ((video_bitrate + audio_bitrate) / 1000.)) as f32
        );
        */
        let pass = [
            "-y",
            "-i",
            old_path_str,
            "-vcodec",
            video_codec,
            "-acodec",
            audio_codec,
            "-vf",
            &scale_arg,
            "-deadline",
            "good",
            "-quality",
            "good",
            "-cpu-used",
            "0",
            "-undershoot-pct",
            "0",
            "-overshoot-pct",
            "0",
            "-b:v",
            &bitrate_arg,
            "-minrate",
            &minrate_arg,
            "-maxrate",
            &maxrate_arg,
            "-b:a",
            &ba_arg,
            "-row-mt",
            "1",
            "-tile-rows",
            "2",
            "-tile-columns",
            "4",
            "-threads",
            "16",
            "-auto-alt-ref",
            "6",
            "-qmax",
            "60",
            "-qmin",
            "1",
            "-g",
            "240",
            "-passlogfile",
            passlogfile
                .as_os_str()
                .to_str()
                .context("missing or bad path")?,
        ];

        command.args(pass);

        command.args([
            "-pass",
            "1",
            "-f",
            new_path.extension().unwrap().to_str().unwrap(),
        ]);
        if cfg!(windows) {
            command.arg("NUL");
        } else {
            command.arg("/dev/null");
        }
        command2.args(pass);
        command2.args([
            "-pass",
            "2",
            new_path
                .as_os_str()
                .to_str()
                .context("missing or bad path")?,
        ]);
        dbg!(&command);
        dbg!(&command2);
        Ok(FFMPEGCommand {
            file_name: path.file_name().unwrap().to_str().unwrap().to_owned(),
            resolution: None,
            duration: Some(duration),
            command: (command, Some(command2)),
            media_type: MediaType::Video,
            target_size: size,
            buff_reader: None,
            exec_handle: None,
            status: EncodingStatus::InProgress,
            passed_pass_1: false,
            progressed_time: 0.,
            progress_bar: None,
        })
    }

    fn create_image(path: &PathBuf, size: u16) -> anyhow::Result<Self> {
        let mut new_path = path.clone();
        new_path.set_extension("webp");
        let mut command = Command::new("ffmpeg");
        command.args(["-progress", "-", "-nostats", "-stats_period", "50ms"]);
        command.args([
            "-y",
            "-i",
            path.as_os_str()
                .to_str()
                .expect("Path dissapeared on unwrap"),
            "-qscale",
            "90",
            "-compression_level",
            "6",
            new_path
                .as_os_str()
                .to_str()
                .expect("Path dissapeared on unwrap"),
        ]);
        Ok(FFMPEGCommand {
            file_name: path.file_name().unwrap().to_str().unwrap().to_owned(),
            resolution: None,
            duration: None,
            command: (command, None),
            media_type: MediaType::Image,
            target_size: size,
            status: EncodingStatus::InProgress,
            exec_handle: None,
            buff_reader: None,
            passed_pass_1: false,
            progressed_time: 0.,
            progress_bar: None,
        })
    }
    fn create_animated_image(_path: &PathBuf) -> anyhow::Result<Self> {
        bail!("")
    }
}

#[derive(PartialEq, Eq, Debug)]
pub enum MediaType {
    Video,
    Audio,
    Image,
    AnimatedImage,
}
#[derive(PartialEq, Eq, Debug)]
pub enum EncodingStatus {
    Finished,
    Failed,
    InProgress,
    NotStarted,
}

async fn parse_ffprobe(path: &PathBuf) -> anyhow::Result<MediaData> {
    let args = [
        "-v",
        "error",
        "-select_streams",
        "v:0",
        "-show_entries",
        "stream=width,height,duration,bit_rate",
        "-of",
        "csv=s=,:p=0",
    ];

    let ffprobe = Command::new("ffprobe")
        .args(args)
        .arg(path)
        .stderr(Stdio::piped())
        .output()
        .await?;
    ffprobe
        .status
        .exit_ok()
        .context("Failed to run ffprobe. Make sure ffprobe is installed and file exists")?;

    let text = std::str::from_utf8(&ffprobe.stdout)?;

    let mem = text.split(',').collect::<Vec<_>>();

    let width = mem.first().and_then(|v| v.parse::<u16>().ok());
    let height = mem.get(1).and_then(|v| v.parse::<u16>().ok());

    let duration = match mem.get(2).and_then(|v| v.parse::<f32>().ok()) {
        Some(d) => d,
        None => {
            let metadata = get_attribute_from_meta("duration", path)
                .await
                .context("can't find duration anywhere")?;
            let res = metadata.parse::<f32>();
            //see if metadatat had seconds directly
            if let Ok(d) = res {
                d
            } else {
                //  try to convert 00:00:00:00 to 0.000s
                if !metadata.contains(":") {
                    return Err(anyhow::anyhow!("can't find duration of media anywhere"));
                } else {
                    let mut res = 0.;
                    let mut iter = metadata.split(':').rev();
                    let secs = iter.next().map(|n| n.parse::<f32>().ok()).flatten();
                    let mins = iter
                        .next()
                        .map(|n| n.parse::<f32>().ok().map(|m| m * 60.))
                        .flatten();
                    let hrs = iter
                        .next()
                        .map(|n| n.parse::<f32>().ok().map(|h| h * 3600.))
                        .flatten();
                    let days = iter
                        .next()
                        .map(|n| n.parse::<f32>().ok().map(|d| d * 24. * 3600.))
                        .flatten();
                    if let Some(s) = secs {
                        res = res + s
                    };
                    if let Some(m) = mins {
                        res = res + m
                    };
                    if let Some(h) = hrs {
                        res = res + h
                    };
                    if let Some(d) = days {
                        res = res + d
                    };
                    res
                }
            }
        }
    };

    dbg!(&duration);
    let old_kbit_rate = mem
        .get(3)
        .and_then(|v| v.parse::<u32>().ok().map(|v| v / 1000));

    let resolution = width.zip(height);

    Ok(MediaData {
        duration,
        resolution,
        old_kbit_rate,
    })
}

async fn get_attribute_from_meta(attr: &str, path: &PathBuf) -> Option<String> {
    let ffprobe = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            &format!("stream={attr}:stream_args={attr}"),
            "-of",
            "csv=s=,:p=0",
        ])
        .arg(path)
        .stderr(Stdio::piped())
        .output()
        .await
        .ok()?;
    ffprobe.status.exit_ok().ok()?;

    std::str::from_utf8(&ffprobe.stdout)
        .ok()
        .map(|v| v.to_string())
}
