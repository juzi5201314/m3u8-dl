# m3u8-dl
Download m3u8 video

# Install
```
cargo install m3u8-dl
```

# Usage
```
#> m3u8-dl --help
Usage: m3u8-dl <url> [--num <num>] [-l <limit>] [--reload] [-t] [-o <output>] [--cache-dir <cache-dir>]

m3u8 downloader

Options:
  --num             max fragments
  -l, --limit       limit the number of concurrent downloads. default: 10
  --reload          do not use cache, force download
  -t, --transcode   transcode to mp4
  -o, --output      output name. `-o test1` // output: test1.ts
  --cache-dir       cache dir
  --help            display usage information
```

# Example
```
// Limit 20 concurrent downloads, output as one.ts, and use ffmpeg to transcode to mp4
// 限制20并发下载，输出为one.ts，并使用ffmpeg转码为mp4
m3u8-dl -l 20 -t -o one https://example.com/index.m3u8
```

# Dependency
`-t` requires `ffmpeg`, if it is not found in $PATH, it will go to FFMPEG_PATH to find it.

`-t` 需要 `ffmpeg`, 如果在$PATH找不到，则会去FFMPEG_PATH寻找。