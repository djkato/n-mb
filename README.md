<a href='https://ko-fi.com/A0A8Q3SVZ' target='_blank'><img height='36' style='border:0px;height:36px;' src='https://storage.ko-fi.com/cdn/kofi4.png?v=3' border='0' alt='Buy Me a Coffee at ko-fi.com' /></a>
# Automatically converts any media file and makes sure its under your limit!
For all those who want to post memes that are just too big and surpass the 25mb free upload limit on discord, this is the app for you!

![nbm usage](https://github.com/djkato/n-mb/assets/25299243/b2531d88-5de1-465f-9bef-d0ad225f06b4)

## This program outputs to following formats:
 - audio codec: opus .ogg
 - video codec: vp9 + opus .webm
 - image codec: vp8 .webp (for gifs too)

## How to install Binary(Windows, Linux):
1. Download binary from [Releases](https://github.com/djkato/n-mb/releases), put into $PATH
2. get ffmpeg for your platform [here](https://ffmpeg.org/download.html), put into $PATH
3. execute anywhere using the `nmb --size/-s <SIZE IN MB> --codec/-c <WEBM/HEVC> --files/-f=<FILE 1>,<FILE 2> . . .` command!

## How to install From Source(Windows, Linux, MacOS):
1. get rustup (cargo, rustc etc) from [here](https://www.rust-lang.org/tools/install)
2. get ffmpeg for your platform [here](https://ffmpeg.org/download.html), put into $PATH
3. run `cargo install n-mb` in your favourite terminal
4. execute anywhere using the `nmb --size/-s <SIZE IN MB> --codec/-c <WEBM/HEVC> --files/-f=<FILE 1>,<FILE 2> . . .` command!

<sub>Thanks for an amazing read on how to optimize vp9 for file sizes deterenkelt, I recommend this read: https://codeberg.org/deterenkelt/Nadeshiko/wiki/Researches%E2%80%89%E2%80%93%E2%80%89VP9-and-overshooting</sub>
