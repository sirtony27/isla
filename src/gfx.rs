//! GPU drawing: Direct2D on a DXGI swap chain hosted by DirectComposition.
//! The swap chain has premultiplied alpha, so everything outside the notch
//! shape is truly transparent and composed by DWM without a redirection bitmap.

use std::mem::ManuallyDrop;

use windows::core::{w, Interface, Result, HSTRING, PCWSTR};
use windows::Win32::Foundation::{HMODULE, HWND};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT, D2D_SIZE_U,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Bitmap1, ID2D1BitmapBrush1, ID2D1DeviceContext, ID2D1Factory,
    ID2D1Factory1, ID2D1Image, ID2D1SolidColorBrush, ID2D1StrokeStyle, D2D1_BITMAP_OPTIONS,
    D2D1_BITMAP_OPTIONS_CANNOT_DRAW, D2D1_BITMAP_OPTIONS_NONE, D2D1_BITMAP_OPTIONS_TARGET,
    D2D1_BITMAP_PROPERTIES1, D2D1_CAP_STYLE_ROUND, D2D1_DEVICE_CONTEXT_OPTIONS_NONE,
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_LINE_JOIN_ROUND, D2D1_STROKE_STYLE_PROPERTIES,
    D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE,
};
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE, D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION,
};
use windows::Win32::Graphics::DirectComposition::{
    DCompositionCreateDevice, IDCompositionDevice, IDCompositionTarget, IDCompositionVisual,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteFontCollection, IDWriteTextFormat,
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD,
    DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_METRICS, DWRITE_TRIMMING,
    DWRITE_TRIMMING_GRANULARITY_CHARACTER, DWRITE_WORD_WRAPPING_NO_WRAP,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    IDXGIAdapter, IDXGIDevice, IDXGIFactory2, IDXGIOutput, IDXGISurface, IDXGISwapChain1,
    DXGI_PRESENT, DXGI_SCALING_STRETCH, DXGI_SWAP_CHAIN_DESC1, DXGI_SWAP_CHAIN_FLAG,
    DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL, DXGI_USAGE_RENDER_TARGET_OUTPUT,
};
use windows_numerics::Matrix3x2;

/// Text formats. Device-independent, so they survive a GPU reset.
pub struct Fonts {
    dw: IDWriteFactory,
    pub clock: IDWriteTextFormat,
    pub title: IDWriteTextFormat,
    pub body: IDWriteTextFormat,
    pub small: IDWriteTextFormat,
    pub label: IDWriteTextFormat,
    /// For lyric lines too long for `label`.
    pub label_small: IDWriteTextFormat,
}

impl Fonts {
    pub fn new() -> Result<Self> {
        unsafe {
            let dw: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            // Windows 11 ships Segoe UI Variable; Windows 10 falls back to Segoe UI.
            let mut collection: Option<IDWriteFontCollection> = None;
            dw.GetSystemFontCollection(&mut collection, false)?;
            let family = match collection {
                Some(c) if has_family(&c, w!("Segoe UI Variable Text")) => "Segoe UI Variable Text",
                _ => "Segoe UI",
            };
            let make = |size: f32, weight: DWRITE_FONT_WEIGHT| -> Result<IDWriteTextFormat> {
                let f = dw.CreateTextFormat(
                    &HSTRING::from(family),
                    None,
                    weight,
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    size,
                    w!(""),
                )?;
                f.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
                f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
                let sign = dw.CreateEllipsisTrimmingSign(&f)?;
                let trimming = DWRITE_TRIMMING {
                    granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER,
                    delimiter: 0,
                    delimiterCount: 0,
                };
                f.SetTrimming(&trimming, &sign)?;
                Ok(f)
            };
            Ok(Self {
                clock: make(13.0, DWRITE_FONT_WEIGHT_SEMI_BOLD)?,
                title: make(15.0, DWRITE_FONT_WEIGHT_SEMI_BOLD)?,
                body: make(13.0, DWRITE_FONT_WEIGHT_NORMAL)?,
                small: make(11.0, DWRITE_FONT_WEIGHT_NORMAL)?,
                label: make(13.0, DWRITE_FONT_WEIGHT_SEMI_BOLD)?,
                label_small: make(11.5, DWRITE_FONT_WEIGHT_SEMI_BOLD)?,
                dw,
            })
        }
    }
}

impl Fonts {
    /// Width in DIPs of `s` on a single line in `format`.
    pub fn width(&self, s: &str, format: &IDWriteTextFormat) -> f32 {
        let wide: Vec<u16> = s.encode_utf16().collect();
        unsafe {
            let Ok(layout) = self.dw.CreateTextLayout(&wide, format, 4096.0, 64.0) else {
                return 0.0;
            };
            let mut m = DWRITE_TEXT_METRICS::default();
            match layout.GetMetrics(&mut m) {
                Ok(()) => m.width,
                Err(_) => 0.0,
            }
        }
    }
}

fn has_family(collection: &IDWriteFontCollection, name: PCWSTR) -> bool {
    let mut index = 0;
    let mut exists = false.into();
    unsafe { collection.FindFamilyName(name, &mut index, &mut exists) }.is_ok() && exists.as_bool()
}

pub struct Gfx {
    _d3d: ID3D11Device,
    pub factory: ID2D1Factory1,
    pub ctx: ID2D1DeviceContext,
    pub brush: ID2D1SolidColorBrush,
    /// Round caps and joins, for line icons.
    pub round: ID2D1StrokeStyle,
    swap: IDXGISwapChain1,
    /// The swap chain's back buffer as a D2D target.
    target: Option<ID2D1Bitmap1>,
    dcomp: IDCompositionDevice,
    _dcomp_target: IDCompositionTarget,
    _visual: IDCompositionVisual,
}

fn bitmap_props(dpi: f32, options: D2D1_BITMAP_OPTIONS) -> D2D1_BITMAP_PROPERTIES1 {
    D2D1_BITMAP_PROPERTIES1 {
        pixelFormat: D2D1_PIXEL_FORMAT {
            format: DXGI_FORMAT_B8G8R8A8_UNORM,
            alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
        },
        dpiX: dpi,
        dpiY: dpi,
        bitmapOptions: options,
        colorContext: ManuallyDrop::new(None),
    }
}

fn create_device(kind: D3D_DRIVER_TYPE) -> Result<ID3D11Device> {
    let mut device = None;
    unsafe {
        D3D11CreateDevice(
            None::<&IDXGIAdapter>,
            kind,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            None,
        )?;
    }
    device.ok_or_else(windows::core::Error::empty)
}

impl Gfx {
    pub fn new(hwnd: HWND, width: u32, height: u32, dpi: f32) -> Result<Self> {
        unsafe {
            // WARP (Windows' software rasterizer) on purpose: a GPU driver costs
            // 30-60 MB of private memory per process and, on hybrid laptops, keeps
            // the discrete GPU awake. Our surface is tiny and drawn only on change;
            // DWM still composes it on the GPU.
            let d3d = create_device(D3D_DRIVER_TYPE_WARP)
                .or_else(|_| create_device(D3D_DRIVER_TYPE_HARDWARE))?;
            let dxgi: IDXGIDevice = d3d.cast()?;
            let dxgi_factory: IDXGIFactory2 = dxgi.GetAdapter()?.GetParent()?;
            let desc = DXGI_SWAP_CHAIN_DESC1 {
                Width: width,
                Height: height,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                BufferCount: 2,
                Scaling: DXGI_SCALING_STRETCH,
                SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
                AlphaMode: DXGI_ALPHA_MODE_PREMULTIPLIED,
                ..Default::default()
            };
            let swap =
                dxgi_factory.CreateSwapChainForComposition(&d3d, &desc, None::<&IDXGIOutput>)?;

            let factory: ID2D1Factory1 =
                D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let ctx = factory
                .CreateDevice(&dxgi)?
                .CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;
            ctx.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            // The base factory method: the D2D 1.1 overload returns ID2D1StrokeStyle1.
            let base: &ID2D1Factory = &factory;
            let round = base.CreateStrokeStyle(
                &D2D1_STROKE_STYLE_PROPERTIES {
                    startCap: D2D1_CAP_STYLE_ROUND,
                    endCap: D2D1_CAP_STYLE_ROUND,
                    dashCap: D2D1_CAP_STYLE_ROUND,
                    lineJoin: D2D1_LINE_JOIN_ROUND,
                    miterLimit: 1.0,
                    ..Default::default()
                },
                None,
            )?;
            let brush = ctx.CreateSolidColorBrush(
                &D2D1_COLOR_F {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                    a: 1.0,
                },
                None,
            )?;

            let dcomp: IDCompositionDevice = DCompositionCreateDevice(&dxgi)?;
            let target = dcomp.CreateTargetForHwnd(hwnd, true)?;
            let visual = dcomp.CreateVisual()?;
            visual.SetContent(&swap)?;
            target.SetRoot(&visual)?;
            dcomp.Commit()?;

            let mut gfx = Self {
                _d3d: d3d,
                factory,
                ctx,
                brush,
                round,
                swap,
                target: None,
                dcomp,
                _dcomp_target: target,
                _visual: visual,
            };
            gfx.bind_target(dpi)?;
            Ok(gfx)
        }
    }

    fn bind_target(&mut self, dpi: f32) -> Result<()> {
        unsafe {
            let surface: IDXGISurface = self.swap.GetBuffer(0)?;
            let props = bitmap_props(
                dpi,
                D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
            );
            let bitmap = self
                .ctx
                .CreateBitmapFromDxgiSurface(&surface, Some(&props))?;
            self.ctx.SetTarget(&bitmap);
            self.ctx.SetDpi(dpi, dpi);
            self.target = Some(bitmap);
        }
        Ok(())
    }

    pub fn resize(&mut self, width: u32, height: u32, dpi: f32) -> Result<()> {
        unsafe {
            self.target = None;
            self.ctx.SetTarget(None::<&ID2D1Image>);
            self.swap.ResizeBuffers(
                2,
                width,
                height,
                DXGI_FORMAT_B8G8R8A8_UNORM,
                DXGI_SWAP_CHAIN_FLAG(0),
            )?;
            self.dcomp.Commit()?;
        }
        self.bind_target(dpi)
    }

    /// Draws one frame. `Present(1)` waits for the next vertical blank, which
    /// paces animations to the monitor's refresh rate without a timer.
    pub fn draw(&self, paint: impl FnOnce(&Gfx)) -> Result<()> {
        unsafe {
            self.ctx.BeginDraw();
            self.ctx.Clear(Some(&D2D1_COLOR_F::default()));
            paint(self);
            self.ctx.EndDraw(None, None)?;
            self.swap.Present(1, DXGI_PRESENT(0)).ok()
        }
    }

    /// An offscreen bitmap that can be drawn into and then drawn from.
    pub fn layer(&self, width: u32, height: u32, dpi: f32) -> Option<ID2D1Bitmap1> {
        let props = bitmap_props(dpi, D2D1_BITMAP_OPTIONS_TARGET);
        let size = D2D_SIZE_U { width, height };
        unsafe { self.ctx.CreateBitmap(size, None, 0, &props) }.ok()
    }

    /// Draws into `layer`, whose left edge sits `offset` DIPs from the window's,
    /// so `paint` keeps using window coordinates.
    pub fn render_layer(
        &self,
        layer: &ID2D1Bitmap1,
        offset: f32,
        paint: impl FnOnce(&Gfx),
    ) -> Result<()> {
        unsafe {
            self.ctx.SetTarget(layer);
            self.ctx.BeginDraw();
            self.ctx.Clear(Some(&D2D1_COLOR_F::default()));
            self.ctx.SetTransform(&Matrix3x2::translation(-offset, 0.0));
            paint(self);
            self.ctx.SetTransform(&Matrix3x2::identity());
            let result = self.ctx.EndDraw(None, None);
            if let Some(target) = &self.target {
                self.ctx.SetTarget(target);
            }
            result
        }
    }

    pub fn art_brush(&self, size: u32, pixels: &[u8]) -> Option<ID2D1BitmapBrush1> {
        let props = bitmap_props(96.0, D2D1_BITMAP_OPTIONS_NONE);
        unsafe {
            let bitmap: ID2D1Bitmap1 = self
                .ctx
                .CreateBitmap(
                    D2D_SIZE_U {
                        width: size,
                        height: size,
                    },
                    Some(pixels.as_ptr().cast()),
                    size * 4,
                    &props,
                )
                .ok()?;
            self.ctx.CreateBitmapBrush(&bitmap, None, None).ok()
        }
    }
}
