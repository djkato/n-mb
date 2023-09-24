use crate::encoder::{EncodingStatus, FFMPEGCommand, MediaType};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::interval;

pub async fn display(commands: Arc<Mutex<Vec<FFMPEGCommand>>>) {
    let mb = MultiProgress::new();
    let sty = ProgressStyle::with_template(
        "{spinner:.blue} {msg} [{elapsed_precise}] {bar:40.cyan/blue} {pos:>7}/{len:7} (ms)",
    )
    .unwrap()
    .tick_strings(&[
        "▏", "▎", "▍", "▌", "▋", "▉", "█", "█", "▉", "▊", "▋", "▌", "▍", "▎", "▏",
    ]);
    //.progress_chars("##-");

    let mut pbs = vec![];
    for (i, command) in commands.lock().await.iter_mut().enumerate() {
        if let Some(dur) = command.duration {
            let pb = mb.add(ProgressBar::new((dur * 100.) as u64));
            pb.set_style(sty.clone());
            pb.set_message("Starting : ");
            pb.tick();
            pbs.push((i, pb));
        }
    }
    let mut spawns = vec![];
    for pb in pbs.into_iter() {
        let commands_mut = commands.clone();
        spawns.push(tokio::spawn(async move {
            let pr = pb.1;
            let mut intv = interval(Duration::from_millis(50));
            loop {
                intv.tick().await;
                let command = commands_mut.lock().await;
                let command = command
                    .get(pb.0)
                    .expect("command for progressbar failed to index");
                match command.status {
                    EncodingStatus::NotStarted => pr.set_message("Starting : "),
                    EncodingStatus::InProgress => match command.media_type {
                        MediaType::Video => match command.passed_pass_1 {
                            true => {
                                pr.set_message(command.file_name.clone() + ": Encoding (Pass 2/2)")
                            }
                            false => {
                                pr.set_message(command.file_name.clone() + ": Encoding (Pass 1/2)")
                            }
                        },
                        _ => pr.set_message(command.file_name.clone() + ": Encoding"),
                    },
                    EncodingStatus::Failed => {
                        pr.set_message(command.file_name.clone() + ": Failed!");
                        pr.set_position(command.duration.unwrap() as u64);
                        pr.finish();
                        break;
                    }
                    EncodingStatus::Finished => {
                        pr.set_message(command.file_name.clone() + ": Finished!");
                        pr.set_position(command.duration.unwrap() as u64);
                        pr.finish();
                        break;
                    }
                };
                pr.set_position((command.progressed_time * 100.) as u64);
            }
        }));
    }

    for spawn in spawns {
        if let Ok(_) = spawn.await {};
    }
    /*
    pb.tick_format("▏▎▍▌▋▊▉██▉▊▋▌▍▎▏");
    pb.show_message = true;
    pb.message("Waiting  : ");
    pb.tick();
    pb.message("Connected: ");

    pb.inc();

    pb.message("Cleaning :");
    pb.tick();
    pb.finish_print(&format!(": Pull complete",));
    mb.println("");
    mb.println("Text lines separate between two sections: ");
    mb.println("");

    mb.listen();

    println!("\nall bars done!\n");
    */
}
