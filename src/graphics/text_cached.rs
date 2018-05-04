use super::*;

pub use gfx_glyph::{FontId, HorizontalAlign, Scale, VerticalAlign};
use gfx_glyph::{self, GlyphPositioner, SectionText, VariedSection};
use rusttype::{point, PositionedGlyph};
use std::borrow::Cow;
use std::f32;

/// Aliased type from `gfx_glyph`.
pub type Layout = gfx_glyph::Layout<gfx_glyph::BuiltInLineBreaker>;

/// Default scale, used as `Scale::uniform(DEFAULT_FONT_SCALE)` when no explicit scale is given.
pub const DEFAULT_FONT_SCALE: f32 = 16.0;

/// A piece of text with optional color, font and font scale information.
/// These options take precedence over any similar field/argument.
/// Can be implicitly constructed from `String`, `(String, Color)`, and `(String, FontId, Scale)`.
#[derive(Clone, Debug)]
pub struct TextFragment {
    /// Text string itself.
    pub text: String,
    /// Fragment's color, defaults to text's color.
    pub color: Option<Color>,
    /// Fragment's font ID, defaults to text's font ID.
    pub font_id: Option<FontId>,
    /// Fragment's scale, defaults to text's scale.
    pub scale: Option<Scale>,
}

impl Default for TextFragment {
    fn default() -> Self {
        TextFragment {
            text: "".into(),
            color: None,
            font_id: None,
            scale: None,
        }
    }
}

impl From<String> for TextFragment {
    fn from(text: String) -> TextFragment {
        TextFragment {
            text,
            ..Default::default()
        }
    }
}

impl From<(String, Color)> for TextFragment {
    fn from(tuple: (String, Color)) -> TextFragment {
        TextFragment {
            text: tuple.0,
            color: Some(tuple.1),
            ..Default::default()
        }
    }
}

impl From<(String, FontId, Scale)> for TextFragment {
    fn from(tuple: (String, FontId, Scale)) -> TextFragment {
        TextFragment {
            text: tuple.0,
            font_id: Some(tuple.1),
            scale: Some(tuple.2),
            ..Default::default()
        }
    }
}

impl<'a> From<&'a str> for TextFragment {
    fn from(text: &'a str) -> TextFragment {
        TextFragment {
            text: text.to_string(),
            ..Default::default()
        }
    }
}

impl<'a> From<(&'a str, Color)> for TextFragment {
    fn from(tuple: (&'a str, Color)) -> TextFragment {
        TextFragment {
            text: tuple.0.to_string(),
            color: Some(tuple.1),
            ..Default::default()
        }
    }
}

impl<'a> From<(&'a str, FontId, Scale)> for TextFragment {
    fn from(tuple: (&'a str, FontId, Scale)) -> TextFragment {
        TextFragment {
            text: tuple.0.to_string(),
            font_id: Some(tuple.1),
            scale: Some(tuple.2),
            ..Default::default()
        }
    }
}

impl From<(Point2, f32)> for DrawParam {
    fn from(tuple: (Point2, f32)) -> DrawParam {
        DrawParam {
            dest: tuple.0,
            rotation: tuple.1,
            ..Default::default()
        }
    }
}

/// Drawable text.
/// Can be either monolithic, or consist of differently-formatted fragments.
#[derive(Clone, Debug)]
pub struct TextCached {
    fragments: Vec<TextFragment>,
    // TODO: make it do something, maybe.
    blend_mode: Option<BlendMode>,
    bounds: Point2,
    layout: Layout,
    font_id: FontId,
    font_scale: Scale,
    cached_string: Option<String>,
    cached_width: Option<u32>,
    cached_height: Option<u32>,
}

impl Default for TextCached {
    fn default() -> Self {
        TextCached {
            fragments: Vec::new(),
            blend_mode: None,
            bounds: Point2::new(f32::INFINITY, f32::INFINITY),
            layout: Layout::default(),
            font_id: FontId::default(),
            font_scale: Scale::uniform(DEFAULT_FONT_SCALE),
            cached_string: None,
            cached_width: None,
            cached_height: None,
        }
    }
}

impl TextCached {
    // TODO: consider ditching context - it's here for consistency's sake, that's it.
    /// Creates a `TextCached` from a `TextFragment`.
    pub fn new<F>(context: &mut Context, fragment: F) -> GameResult<TextCached>
    where
        F: Into<TextFragment>,
    {
        let mut text = TextCached::new_empty(context)?;
        text.add_fragment(fragment);
        Ok(text)
    }

    /// Creates an empty `TextCached`.
    pub fn new_empty(context: &mut Context) -> GameResult<TextCached> {
        Ok(TextCached::default())
    }

    /// Appends a `TextFragment`.
    pub fn add_fragment<F>(&mut self, fragment: F) -> &mut TextCached
    where
        F: Into<TextFragment>,
    {
        self.fragments.push(fragment.into());
        self.invalidate_caches();
        self
    }

    /// Replaces a `TextFragment` without having to rebuild the entire `TextCached`.
    /// Useful for things like animating specific words, or highlighting them on mouseover.
    pub fn replace_fragment<F>(&mut self, old_index: usize, new_fragment: F) -> &mut TextCached
    where
        F: Into<TextFragment>,
    {
        self.fragments[old_index] = new_fragment.into();
        self.invalidate_caches();
        self
    }

    /// Specifies rectangular dimensions to try and fit contents inside of, by wrapping.
    /// Alignment within bounds can be changed by passing a `Layout`; defaults to top left corner.
    pub fn set_bounds(&mut self, bounds: Point2, layout: Option<Layout>) -> &mut TextCached {
        self.bounds = bounds;
        if self.bounds.x == f32::INFINITY {
            // Layouts don't make any sense if we don't wrap text at all.
            self.layout = Layout::default();
        } else {
            if let Some(layout) = layout {
                self.layout = layout;
            }
        }
        self.invalidate_caches();
        self
    }

    /// Specifies text's font and font scale; used for fragments that don't have their own.
    pub fn set_font(&mut self, font_id: FontId, font_scale: Scale) -> &mut TextCached {
        self.font_id = font_id;
        self.font_scale = font_scale;
        self.invalidate_caches();
        self
    }

    fn generate_varied_section<'a>(
        &'a self,
        context: &Context,
        relative_dest: Point2,
        color: Option<Color>,
    ) -> VariedSection<'a> {
        let mut sections = Vec::new();
        for fragment in &self.fragments {
            let color = match fragment.color {
                Some(c) => c,
                None => match color {
                    Some(c) => c,
                    None => get_color(context),
                },
            };
            let font_id = match fragment.font_id {
                Some(font_id) => font_id,
                None => self.font_id,
            };
            let scale = match fragment.scale {
                Some(scale) => scale,
                None => self.font_scale,
            };
            sections.push(SectionText {
                text: &fragment.text,
                color: <[f32; 4]>::from(color),
                font_id,
                scale,
            });
        }
        let relative_dest = (
            {
                // This positions text within bounds with relative_dest being to the left, always.
                let mut dest_x = relative_dest.x;
                if self.bounds.x != f32::INFINITY {
                    use gfx_glyph::Layout::Wrap;
                    if let Wrap { h_align, .. } = self.layout {
                        match h_align {
                            HorizontalAlign::Center => dest_x += self.bounds.x * 0.5,
                            HorizontalAlign::Right => dest_x += self.bounds.x,
                            _ => (),
                        }
                    }
                }
                dest_x
            },
            relative_dest.y,
        );
        VariedSection {
            screen_position: relative_dest,
            bounds: (self.bounds.x, self.bounds.y),
            //z: f32,
            layout: self.layout,
            text: sections,
            ..Default::default()
        }
    }

    fn invalidate_caches(&mut self) {
        self.cached_string = None;
        self.cached_width = None;
        self.cached_height = None;
    }

    fn calculate_dimensions(&mut self, context: &Context) -> (u32, u32) {
        let mut max_width = 0;
        let mut max_height = 0;
        {
            let varied_section = self.generate_varied_section(context, Point2::new(0.0, 0.0), None);
            let glyphed_section_texts = self.layout
                .calculate_glyphs(context.gfx_context.glyph_brush.fonts(), &varied_section);
            for glyphed_section_text in &glyphed_section_texts {
                let &gfx_glyph::GlyphedSectionText(ref positioned_glyphs, ..) =
                    glyphed_section_text;
                for positioned_glyph in positioned_glyphs {
                    if let Some(rect) = positioned_glyph.pixel_bounding_box() {
                        if rect.max.x > max_width {
                            max_width = rect.max.x;
                        }
                        if rect.max.y > max_height {
                            max_height = rect.max.y;
                        }
                    }
                }
            }
        }
        let (width, height) = (max_width as u32, max_height as u32);
        self.cached_width = Some(width);
        self.cached_height = Some(height);
        (width, height)
    }

    // TODO: doc better
    /// Calculates the width
    pub fn width(&mut self, context: &Context) -> u32 {
        match self.cached_width {
            Some(w) => w,
            None => self.calculate_dimensions(context).0,
        }
    }

    // TODO: doc better
    /// Calculates the height
    pub fn height(&mut self, context: &Context) -> u32 {
        match self.cached_height {
            Some(h) => h,
            None => self.calculate_dimensions(context).1,
        }
    }

    /// Returns the string that the text represents.
    pub fn contents(&mut self) -> String {
        if let Some(ref string) = self.cached_string {
            return string.clone();
        }
        let string = self.fragments
            .iter()
            .fold("".to_string(), |acc, frg| format!("{}{}", acc, frg.text));
        self.cached_string = Some(string.clone());
        string
    }

    // TODO: figure out how to use font metrics to make it behave as `DrawParam::offset` does.
    /// Queues the `TextCached` to be drawn by `draw_queued()`.
    /// This is much more efficient than using `graphics::draw()` or equivalent.
    /// `relative_dest` is relative to the `DrawParam::dest` passed to `draw_queued()`.
    /// Note, any `TextCached` drawn via `graphics::draw()` will also draw the queue.
    pub fn queue(&self, context: &mut Context, relative_dest: Point2, color: Option<Color>) {
        let varied_section = self.generate_varied_section(context, relative_dest, color);
        context.gfx_context.glyph_brush.queue(varied_section);
    }

    /// Exposes `gfx_glyph`'s `GlyphBrush::queue()` and `GlyphBrush::queue_custom_layout()`,
    /// in case `ggez`' API is insufficient.
    pub fn queue_raw<'a, S, G>(context: &mut Context, section: S, custom_layout: Option<&G>)
    where
        S: Into<Cow<'a, VariedSection<'a>>>,
        G: GlyphPositioner,
    {
        let brush = &mut context.gfx_context.glyph_brush;
        match custom_layout {
            Some(layout) => brush.queue_custom_layout(section, layout),
            None => brush.queue(section),
        }
    }

    /// Draws all of `queue()`d `TextCached`.
    /// This is much more efficient than using `graphics::draw()` or equivalent.
    /// `DrawParam` apply to everything in the queue.
    /// Offset and color are ignored - specify them in `queue()` instead.
    pub fn draw_queued<D>(context: &mut Context, param: D) -> GameResult<()>
    where
        D: Into<DrawParam>,
    {
        let param: DrawParam = param.into();
        type Mat4 = na::Matrix4<f32>;
        type Vec3 = na::Vector3<f32>;

        let (offset_x, offset_y) = (-1.0, 1.0);
        let (screen_w, screen_h) = (
            context.gfx_context.screen_rect.w,
            context.gfx_context.screen_rect.h,
        );
        let (aspect, aspect_inv) = (screen_h / screen_w, screen_w / screen_h);
        let m_aspect = Mat4::new_nonuniform_scaling(&Vec3::new(1.0, aspect_inv, 1.0));
        let m_aspect_inv = Mat4::new_nonuniform_scaling(&Vec3::new(1.0, aspect, 1.0));
        let m_scale = Mat4::new_nonuniform_scaling(&Vec3::new(param.scale.x, param.scale.y, 1.0));
        let m_shear = Mat4::new(
            1.0,
            -param.shear.x,
            0.0,
            0.0,
            -param.shear.y,
            1.0,
            0.0,
            0.0,
            0.0,
            0.0,
            1.0,
            0.0,
            0.0,
            0.0,
            0.0,
            1.0,
        );
        let m_rotation = Mat4::new_rotation(-param.rotation * Vec3::z());
        let m_offset = Mat4::new_translation(&Vec3::new(offset_x, offset_y, 0.0));
        let m_offset_inv = Mat4::new_translation(&Vec3::new(-offset_x, -offset_y, 0.0));
        let m_translate = Mat4::new_translation(&Vec3::new(
            2.0 * param.dest.x / screen_w,
            2.0 * -param.dest.y / screen_h,
            0.0,
        ));

        let m_transform = m_translate * m_offset * m_aspect * m_rotation * m_scale * m_shear
            * m_aspect_inv * m_offset_inv;

        let (encoder, render_tgt, depth_view) = (
            &mut context.gfx_context.encoder,
            &context.gfx_context.screen_render_target,
            &context.gfx_context.depth_view,
        );
        context.gfx_context.glyph_brush.draw_queued_with_transform(
            m_transform.into(),
            encoder,
            render_tgt,
            depth_view,
        )?;
        Ok(())
    }
}

impl Drawable for TextCached {
    fn draw_ex(&self, ctx: &mut Context, param: DrawParam) -> GameResult<()> {
        self.queue(ctx, param.offset, param.color);
        TextCached::draw_queued(ctx, param)
    }

    fn set_blend_mode(&mut self, mode: Option<BlendMode>) {
        self.blend_mode = mode;
    }

    fn get_blend_mode(&self) -> Option<BlendMode> {
        self.blend_mode
    }
}
