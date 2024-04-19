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
        // dbg!(&command);
        // dbg!(&command2);
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
    let ffprobe = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height,duration,bit_rate",
            "-of",
            "csv=s=,:p=0",
        ])
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

    let width = mem.get(0).and_then(|v| v.parse::<u16>().ok());
    let height = mem.get(1).and_then(|v| v.parse::<u16>().ok());
    let duration = mem
        .get(2)
        .and_then(|v| v.parse::<f32>().ok())
        .context("missing duration")?;
    let old_kbit_rate = mem
        .get(3)
        .and_then(|v| v.parse::<u32>().ok().and_then(|v| Some(v / 1000)));

    let resolution = width.zip(height);
    // if let Ok(dur) = parse_duration(text) {
    //     duration = dur
    // } else {
    //     bail!("FFProbe missing duration in media. Is file corrupted or non-existent?")
    // }
    // let old_kbit_rate = parse_bitrate(text).ok();
    //
    // let mut resolution = None;
    // if text.contains("Stream") {
    //     resolution = parse_resolution(text).ok();
    // }
    //
    Ok(MediaData {
        duration,
        resolution,
        old_kbit_rate,
    })
}

fn parse_duration(text: &str) -> anyhow::Result<f32> {
    let text = text[text.find("Duration").unwrap()..].to_owned();
    let dur_text = text[text
        .find(':')
        .context("something wrong with the ffprobe output")?
        + 2
        ..text
            .find(',')
            .context("something wrong with the ffprobe output")?]
        .to_owned();
    let durs_text: Vec<&str> = dur_text.split(':').collect();
    let mut durs_text_iter = durs_text.into_iter();
    let h = durs_text_iter
        .next()
        .context("something wrong with the ffprobe output")?
        .parse::<f32>()?;
    let m = durs_text_iter
        .next()
        .context("something wrong with the ffprobe output")?
        .parse::<f32>()?;
    let s = durs_text_iter
        .next()
        .context("something wrong with the ffprobe output")?
        .parse::<f32>()?;
    Ok(h * 60. * 60. + m * 60. + s)
}

fn parse_bitrate(text: &str) -> anyhow::Result<u16> {
    let text = text[text.find("bitrate").unwrap()..].to_owned();
    let bitrate_text = text[9..text.find('/').unwrap() - 2].to_owned();

    Ok(bitrate_text.parse::<u16>()?)
}

fn parse_resolution(text: &str) -> anyhow::Result<(u16, u16)> {
    let text = text[text.find("Stream").unwrap()..].to_owned();
    let sar_i = text
        .find("[SAR ")
        .context("something wrong with the ffprobe output")?
        - 1;

    let rb_b4_sar_i = text[..sar_i]
        .rfind(',')
        .context("something wrong with the ffprobe output")?
        + 1;

    let res_text = text[rb_b4_sar_i..sar_i].to_owned();
    let res_text = res_text.trim().to_owned();

    let width = res_text[..res_text
        .find('x')
        .context("something wrong with ffprobe output")?]
        .to_owned()
        .parse::<u16>()?;

    let height = res_text[res_text
        .find('x')
        .context("something wrong with ffprobe output")?
        + 1..]
        .to_owned()
        .parse::<u16>()?;

    Ok((width, height))
}
