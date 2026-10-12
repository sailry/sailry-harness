// Modified by Longbridge for gpui-fast.
//! Renderers for the extra surfaces of a window that uses window composition
//! (zed-industries/zed#62379).
//!
//! A platform that stacks GPUI overlays as surfaces of their own (Wayland
//! subsurfaces) draws each with a renderer of its own, created here from the
//! window's renderer so the two share a device and a sprite atlas.

use crate::wgpu_renderer::create_surface;
use crate::{WgpuRenderer, WgpuSurfaceConfig};
use raw_window_handle::HasWindowHandle;
use std::rc::Rc;

impl WgpuRenderer {
    /// Creates a renderer for another surface of the same window. It shares
    /// this renderer's device and sprite atlas, so atlas tiles rasterized for
    /// the window are valid on every surface.
    pub fn new_sharing_atlas<W>(
        &self,
        window: &W,
        config: WgpuSurfaceConfig,
    ) -> anyhow::Result<Self>
    where
        W: HasWindowHandle,
    {
        let gpu_context = self
            .context
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("renderer has no shared GPU context"))?;
        let window_handle = window
            .window_handle()
            .map_err(|e| anyhow::anyhow!("Failed to get window handle: {e}"))?;
        let context_ref = gpu_context.borrow();
        let context = context_ref
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("GPU context is not initialized"))?;
        let surface = create_surface(&context.instance, window_handle.as_raw())?;
        context.check_compatible_with_surface(&surface)?;
        let mut renderer = Self::new_internal(
            Some(Rc::clone(gpu_context)),
            context,
            surface,
            config,
            self.compositor_gpu,
            self.atlas.clone(),
        )?;
        renderer.set_subpixel_layout(self.is_bgr);
        Ok(renderer)
    }

    /// Moves this renderer onto another surface of the same GPU context,
    /// keeping its pipelines and sprite atlas, which are much slower to create
    /// than a surface.
    pub fn replace_surface_sharing_context<W>(
        &mut self,
        window: &W,
        config: WgpuSurfaceConfig,
    ) -> anyhow::Result<()>
    where
        W: HasWindowHandle,
    {
        let gpu_context = Rc::clone(
            self.context
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("renderer has no shared GPU context"))?,
        );
        let context_ref = gpu_context.borrow();
        let context = context_ref
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("GPU context is not initialized"))?;
        self.replace_surface(window, config, &context.instance)
    }
}
