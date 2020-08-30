use argh::FromArgs;

#[derive(FromArgs)]
/// m3u8 downloader
pub struct Args {
    #[argh(positional)]
    pub url: String,

    #[argh(option)]
    /// max fragments
    pub num: Option<usize>,

    #[argh(option, default = "10", short = 'l')]
    /// limit the number of concurrent downloads. default: 10
    pub limit: usize,

    #[argh(switch)]
    /// do not use cache, force download
    pub reload: bool,

    #[argh(switch, short = 't')]
    /// transcode to mp4
    pub transcode: bool,

    #[argh(option, short = 'o')]
    /// output name.
    /// `-o test1`
    /// // output: test1.ts
    pub output: Option<String>,

    #[argh(option, default = "std::env::temp_dir().to_str().unwrap().to_owned()")]
    /// cache dir
    pub cache_dir: String
}