use skia_safe::{Canvas, Picture, PictureRecorder, Point, Rect};

use crate::error::RenderError;

pub struct RecordedLayer {
    picture: Picture,
    bounds: Rect,
}

impl RecordedLayer {
    pub fn record(bounds: Rect, content: impl FnOnce(&Canvas)) -> Result<Self, RenderError> {
        let mut recorder = PictureRecorder::new();
        {
            let canvas = recorder.begin_recording(bounds, false);
            content(canvas);
        }
        let picture = recorder
            .finish_recording_as_picture(Some(&bounds))
            .ok_or_else(|| RenderError::Backend("静态图层录制失败".to_string()))?;
        Ok(Self { picture, bounds })
    }

    pub fn bounds(&self) -> Rect {
        self.bounds
    }

    pub fn picture(&self) -> &Picture {
        &self.picture
    }

    pub fn draw<'c>(&self, canvas: &'c Canvas) -> &'c Canvas {
        canvas.draw_picture(&self.picture, None, None)
    }

    pub fn draw_at<'c>(&self, canvas: &'c Canvas, offset: Point) -> &'c Canvas {
        canvas.save();
        canvas.translate(offset);
        canvas.draw_picture(&self.picture, None, None);
        canvas.restore();
        canvas
    }

    pub fn draw_scaled<'c>(&self, canvas: &'c Canvas, scale: f32) -> &'c Canvas {
        canvas.save();
        canvas.scale((scale, scale));
        canvas.draw_picture(&self.picture, None, None);
        canvas.restore();
        canvas
    }
}

impl std::fmt::Debug for RecordedLayer {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RecordedLayer")
            .field("bounds", &self.bounds)
            .field("unique_id", &self.picture.unique_id())
            .finish()
    }
}
