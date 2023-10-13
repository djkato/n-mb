# Automatically converts any media file and makes sure its under your limit!
For all those who want to post memes that are just too big and surpass the 25mb free upload limit on discord, this is the app for you!

## This program outputs to following formats:
 - audio codec: opus .ogg
 - video codec: vp9 + opus .webm
 - image codec: vp8 .webp (for gifs too)

##How to install Binary(Windows, Linux):
1. Download binary from [Releases](https://github.com/djkato/n-mb/releases), put into $PATH
2. get ffmpeg for your platform [here](https://ffmpeg.org/download.html), put into $PATH
3. execute anywhere using the `nmb --size/-s <SIZE IN MB> --codec/-c <WEBM/HEVC> --files/-f=<FILE 1>,<FILE 2> . . .` command!

## How to install From Source(Windows, Linux, MacOS):
1. get rustup (cargo, rustc etc) from [here](https://www.rust-lang.org/tools/install)
2. get ffmpeg for your platform [here](https://ffmpeg.org/download.html), put into $PATH
3. run `cargo install n-mb` in your favourite terminal
4. execute anywhere using the `nmb --size/-s <SIZE IN MB> --codec/-c <WEBM/HEVC> --files/-f=<FILE 1>,<FILE 2> . . .` command!

<sub>Thanks for an amazing read on how to optimize vp9 for file sizes deterenkelt, I recommend this read: https://codeberg.org/deterenkelt/Nadeshiko/wiki/Researches%E2%80%89%E2%80%93%E2%80%89VP9-and-overshooting</sub>
