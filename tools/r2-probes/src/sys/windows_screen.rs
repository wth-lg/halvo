use std::{
    sync::mpsc,
    time::{Duration, Instant},
};
use windows::core::{factory, w, IInspectable, Interface, Result as WResult, HSTRING};
use windows::Foundation::TypedEventHandler;
use windows::Globalization::Language;
use windows::Graphics::Capture::*;
use windows::Graphics::DirectX::{Direct3D11::IDirect3DDevice, DirectXPixelFormat};
use windows::Graphics::Imaging::{BitmapAlphaMode, BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine;
use windows::Security::Cryptography::CryptographicBuffer;
use windows::Wdk::System::SystemServices::RtlGetVersion;
use windows::Win32::Foundation::{HMODULE, POINT};
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP};
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::Graphics::Gdi::{MonitorFromPoint, MONITOR_DEFAULTTOPRIMARY};
use windows::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD};
use windows::Win32::System::SystemInformation::OSVERSIONINFOW;
use windows::Win32::System::WinRT::Direct3D11::{
    CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess,
};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;

use crate::frame::Frame;

fn s<T>(what: &str, r: WResult<T>) -> Result<T, String> {
    r.map_err(|e| format!("{what}: {:#010x} {}", e.code().0, e.message()))
}

pub fn capture_with_adapter(timeout_ms: u64) -> Result<(Frame, String), String> {
    let (mut device, mut context, mut driver, mut last) = (None, None, "", String::new());
    for (kind, name) in [
        (D3D_DRIVER_TYPE_HARDWARE, "hardware"),
        (D3D_DRIVER_TYPE_WARP, "warp"),
    ] {
        let made = unsafe {
            D3D11CreateDevice(
                None,
                kind,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
        };
        match made {
            Ok(()) => {
                driver = name;
                break;
            }
            Err(e) => last = format!("{last} {name}: {e}"),
        }
    }
    let (Some(device), Some(context)) = (device, context) else {
        return Err(format!("D3D11CreateDevice:{last}"));
    };
    let dxgi: IDXGIDevice = s("IDXGIDevice", device.cast())?;
    let adapter = unsafe { dxgi.GetAdapter().and_then(|a| a.GetDesc()) }
        .map(|d| {
            String::from_utf16_lossy(&d.Description)
                .trim_end_matches('\0')
                .to_string()
        })
        .unwrap_or_default();
    let winrt: IDirect3DDevice = s(
        "IDirect3DDevice",
        unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi) }.and_then(|i| i.cast()),
    )?;
    let interop = s(
        "interop factory",
        factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>(),
    )?;
    let monitor = unsafe { MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY) };
    let item: GraphicsCaptureItem = s("CreateForMonitor", unsafe {
        interop.CreateForMonitor(monitor)
    })?;
    let size = s("Size", item.Size())?;
    let format = DirectXPixelFormat::B8G8R8A8UIntNormalized;
    let pool = s(
        "CreateFreeThreaded",
        Direct3D11CaptureFramePool::CreateFreeThreaded(&winrt, format, 1, size),
    )?;
    let (tx, rx) = mpsc::channel::<()>();
    let handler =
        TypedEventHandler::<Direct3D11CaptureFramePool, IInspectable>::new(move |_, _| {
            let _ = tx.send(());
            Ok(())
        });
    let token = s("FrameArrived", pool.FrameArrived(&handler))?;
    let session = s("CreateCaptureSession", pool.CreateCaptureSession(&item))?;
    let _ = session.SetIsCursorCaptureEnabled(false);
    let _ = session.SetIsBorderRequired(false);
    let frame = s("StartCapture", session.StartCapture())
        .and_then(|()| {
            rx.recv_timeout(Duration::from_millis(timeout_ms))
                .map_err(|_| format!("no frame in {timeout_ms} ms"))
        })
        .and_then(|()| s("TryGetNextFrame", pool.TryGetNextFrame()))
        .and_then(|f| copy(&device, &context, &f));
    let _ = (
        pool.RemoveFrameArrived(token),
        session.Close(),
        pool.Close(),
    );
    frame.map(|f| (f, format!("{driver}: {adapter}")))
}

fn copy(
    device: &ID3D11Device,
    context: &ID3D11DeviceContext,
    frame: &Direct3D11CaptureFrame,
) -> Result<Frame, String> {
    let access: IDirect3DDxgiInterfaceAccess =
        s("Surface", frame.Surface().and_then(|x| x.cast()))?;
    let texture: ID3D11Texture2D = s("GetInterface", unsafe { access.GetInterface() })?;
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    unsafe { texture.GetDesc(&mut desc) };
    desc.Usage = D3D11_USAGE_STAGING;
    desc.BindFlags = 0;
    desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
    desc.MiscFlags = 0;
    let mut staging = None;
    s("CreateTexture2D", unsafe {
        device.CreateTexture2D(&desc, None, Some(&mut staging))
    })?;
    let staging = staging.ok_or("no staging texture")?;
    let mut map = D3D11_MAPPED_SUBRESOURCE::default();
    unsafe { context.CopyResource(&staging, &texture) };
    s("Map", unsafe {
        context.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut map))
    })?;
    let (width, height, pitch) = (
        desc.Width as usize,
        desc.Height as usize,
        map.RowPitch as usize,
    );
    let mut bgra = Vec::with_capacity(width * height * 4);
    for y in 0..height {
        let row = unsafe {
            std::slice::from_raw_parts((map.pData as *const u8).add(y * pitch), width * 4)
        };
        bgra.extend_from_slice(row);
    }
    unsafe { context.Unmap(&staging, 0) };
    Ok(Frame {
        width,
        height,
        bgra,
    })
}

pub fn ocr_languages() -> Vec<String> {
    OcrEngine::AvailableRecognizerLanguages()
        .map(|v| {
            v.into_iter()
                .filter_map(|l| l.LanguageTag().ok())
                .map(|t| t.to_string())
                .collect()
        })
        .unwrap_or_default()
}

pub fn recognize(
    frame: &Frame,
    language_tag: Option<&str>,
    cpu: fn() -> f64,
) -> Result<(Vec<String>, f64, f64), String> {
    if frame.bgra.len() != frame.width * frame.height * 4 {
        return Err("bgra length".into());
    }
    let buffer = s(
        "CreateFromByteArray",
        CryptographicBuffer::CreateFromByteArray(&frame.bgra),
    )?;
    let bitmap = s(
        "CreateCopyWithAlphaFromBuffer",
        SoftwareBitmap::CreateCopyWithAlphaFromBuffer(
            &buffer,
            BitmapPixelFormat::Bgra8,
            frame.width as i32,
            frame.height as i32,
            BitmapAlphaMode::Premultiplied,
        ),
    )?;
    let engine = match language_tag {
        Some(tag) => Language::CreateLanguage(&HSTRING::from(tag))
            .and_then(|l| OcrEngine::TryCreateFromLanguage(&l)),
        None => OcrEngine::TryCreateFromUserProfileLanguages(),
    };
    let engine = s("OcrEngine (a null engine: language not installed)", engine)?;
    let cpu_start = cpu();
    let started = Instant::now();
    let result = s(
        "RecognizeAsync",
        engine.RecognizeAsync(&bitmap).and_then(|op| op.join()),
    )?;
    let ms = started.elapsed().as_secs_f64() * 1000.0;
    let cpu_ms = cpu() - cpu_start;
    let lines = s("Lines", result.Lines())?;
    Ok((
        lines
            .into_iter()
            .map(|l| l.Text().map(|t| t.to_string()).unwrap_or_default())
            .collect(),
        ms,
        cpu_ms,
    ))
}

pub fn windows_build() -> String {
    let mut v = OSVERSIONINFOW {
        dwOSVersionInfoSize: size_of::<OSVERSIONINFOW>() as u32,
        ..Default::default()
    };
    let _ = unsafe { RtlGetVersion(&mut v) };
    let (mut ubr, mut n) = (0u32, 4u32);
    let key = w!("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion");
    let _ = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            key,
            w!("UBR"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut ubr as *mut u32 as *mut _),
            Some(&mut n),
        )
    };
    format!(
        "{}.{}.{}.{}",
        v.dwMajorVersion, v.dwMinorVersion, v.dwBuildNumber, ubr
    )
}
