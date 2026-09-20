use glam::Vec2;

const MIN_ZOOM: f32 = 1.0;
const MAX_ZOOM: f32 = 4.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ResetMode {
    Fit,
    ActualSize,
}

pub struct ViewportController {
    pub zoom: f32,
    pub pan: Vec2,
    pub fit_scale: f32,
    pub viewport_size: Vec2,
    pub content_size: Vec2,
    pub sensitivity: f32,
}

impl ViewportController {
    pub fn new(content_size: Vec2) -> Self {
        Self { zoom: 1.0, pan: Vec2::ZERO, fit_scale: 1.0, viewport_size: Vec2::ONE, content_size, sensitivity: 0.1 }
    }

    pub fn resize(&mut self, viewport_size: Vec2) {
        self.viewport_size = viewport_size.max(Vec2::ONE);
        self.fit_scale = (self.viewport_size.x / self.content_size.x)
            .min(self.viewport_size.y / self.content_size.y)
            .max(0.001);
        self.zoom = self.zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        self.clamp_pan();
    }

    pub fn reset(&mut self, mode: ResetMode) {
        self.zoom = match mode { ResetMode::Fit => MIN_ZOOM, ResetMode::ActualSize => (1.0 / self.fit_scale).clamp(MIN_ZOOM, MAX_ZOOM) };
        self.pan = Vec2::ZERO;
        self.clamp_pan();
    }

    pub fn zoom_at(&mut self, wheel_delta: f32, cursor: Vec2) {
        let old_scale = self.scale();
        let factor = (wheel_delta * self.sensitivity).exp();
        let new_zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        let new_scale = self.fit_scale * new_zoom;
        let content_point = (cursor - self.viewport_size * 0.5 - self.pan) / old_scale;
        self.zoom = new_zoom;
        self.pan = cursor - self.viewport_size * 0.5 - content_point * new_scale;
        self.clamp_pan();
    }

    /// Toolbar zoom preserves the content at the viewer center.
    pub fn set_zoom(&mut self, zoom: f32) {
        if !zoom.is_finite() { return; }
        let old_zoom = self.zoom;
        self.zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        self.pan *= self.zoom / old_zoom;
        self.clamp_pan();
    }

    pub fn actual_zoom(&self, multiple: f32) -> Option<f32> {
        let zoom = multiple / self.fit_scale;
        (zoom.is_finite() && (MIN_ZOOM..=MAX_ZOOM).contains(&zoom)).then_some(zoom)
    }

    pub fn pan_by(&mut self, delta: Vec2) {
        self.pan += delta;
        self.clamp_pan();
    }

    pub fn content_position_at(&self, cursor: Vec2) -> Vec2 {
        let position = (cursor - self.viewport_size * 0.5 - self.pan) / self.scale() + self.content_size * 0.5;
        position.clamp(Vec2::ZERO, self.content_size)
    }

    pub fn scale(&self) -> f32 { self.fit_scale * self.zoom }

    pub fn normalized_transform(&self) -> [f32; 4] {
        let scale = self.scale();
        [
            self.content_size.x * scale / self.viewport_size.x,
            self.content_size.y * scale / self.viewport_size.y,
            self.pan.x / self.viewport_size.x,
            self.pan.y / self.viewport_size.y,
        ]
    }

    fn clamp_pan(&mut self) {
        let scaled = self.content_size * self.scale();
        let excess = ((scaled - self.viewport_size) * 0.5).max(Vec2::ZERO);
        self.pan = self.pan.clamp(-excess, excess);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom_cannot_shrink_below_fit() {
        let mut viewport = ViewportController::new(Vec2::new(1920.0, 1080.0));
        viewport.resize(Vec2::new(1280.0, 720.0));
        viewport.zoom_at(-100.0, Vec2::ZERO);

        assert_eq!(viewport.zoom, MIN_ZOOM);
        assert_eq!(viewport.scale(), viewport.fit_scale);
    }

    #[test]
    fn actual_size_reset_still_fits_large_viewport() {
        let mut viewport = ViewportController::new(Vec2::new(800.0, 600.0));
        viewport.resize(Vec2::new(1600.0, 1200.0));
        viewport.reset(ResetMode::ActualSize);

        assert_eq!(viewport.zoom, MIN_ZOOM);
    }

    #[test]
    fn zoom_is_limited_to_four_times_fit() {
        let mut viewport = ViewportController::new(Vec2::new(1920.0, 1080.0));
        viewport.resize(Vec2::new(1280.0, 720.0));
        viewport.zoom_at(100.0, Vec2::ZERO);

        assert_eq!(viewport.zoom, MAX_ZOOM);
    }

    #[test]
    fn content_position_maps_view_center_to_content_center() {
        let mut viewport = ViewportController::new(Vec2::new(1920.0, 1080.0));
        viewport.resize(Vec2::new(1280.0, 720.0));

        assert_eq!(viewport.content_position_at(Vec2::new(640.0, 360.0)), Vec2::new(960.0, 540.0));
    }
}
