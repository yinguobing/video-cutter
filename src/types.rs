use std::path::PathBuf;

/// DNxHD/DNxHR encoding profile presets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DnxProfile {
    #[allow(non_camel_case_types)]
    DnxHR_LB,  // 36 Mbps 1080p
    #[allow(non_camel_case_types)]
    DnxHR_SQ,  // 60 Mbps 1080p
    #[allow(non_camel_case_types)]
    DnxHR_HQ,  // 110 Mbps 1080p (default)
    #[allow(non_camel_case_types)]
    DnxHR_HQX, // 175 Mbps 1080p
}

impl DnxProfile {
    pub fn label(&self) -> &'static str {
        match self {
            Self::DnxHR_LB => "DNxHR LB",
            Self::DnxHR_SQ => "DNxHR SQ",
            Self::DnxHR_HQ => "DNxHR HQ",
            Self::DnxHR_HQX => "DNxHR HQX",
        }
    }

    pub fn ffmpeg_profile(&self) -> &'static str {
        match self {
            Self::DnxHR_LB => "dnxhr_lb",
            Self::DnxHR_SQ => "dnxhr_sq",
            Self::DnxHR_HQ => "dnxhr_hq",
            Self::DnxHR_HQX => "dnxhr_hqx",
        }
    }
}

/// Information about the loaded video file.
#[derive(Debug, Clone)]
pub struct VideoInfo {
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub duration: f64,
}

/// Parameters for DNxHD/DNxHR export.
#[derive(Debug, Clone)]
pub struct ExportParams {
    pub profile: DnxProfile,
    pub output_path: Option<PathBuf>,
    pub keep_resolution: bool,
}

impl Default for ExportParams {
    fn default() -> Self {
        Self {
            profile: DnxProfile::DnxHR_HQ,
            output_path: None,
            keep_resolution: true,
        }
    }
}

/// The main project state.
#[derive(Debug, Clone)]
pub struct Project {
    pub source_path: Option<PathBuf>,
    pub video_info: Option<VideoInfo>,
    pub in_point: Option<f64>,
    pub out_point: Option<f64>,
    pub export_params: ExportParams,
}

impl Default for Project {
    fn default() -> Self {
        Self {
            source_path: None,
            video_info: None,
            in_point: None,
            out_point: None,
            export_params: ExportParams::default(),
        }
    }
}

impl Project {
    /// Duration of the selected segment in seconds.
    pub fn segment_duration(&self) -> Option<f64> {
        match (self.in_point, self.out_point, self.video_info.as_ref()) {
            (Some(i), Some(o), _) => Some(o - i),
            (Some(i), None, Some(info)) => Some(info.duration - i),
            (None, Some(o), _) => Some(o),
            _ => None,
        }
    }
}

/// A saved export segment with defined in/out points.
#[derive(Debug, Clone)]
pub struct Segment {
    pub in_point: f64,
    pub out_point: f64,
}

impl Segment {
    pub fn duration(&self) -> f64 {
        self.out_point - self.in_point
    }
}
