#[allow(dead_code)]
#[path = "hosted/ink.rs"]
mod ink;

use ink::{ink_reading, ink_runs};

/// Rows of `height` with ink on each `(first, end)` span.
fn rows(height: usize, spans: &[(usize, usize)]) -> Vec<bool> {
    (0..height)
        .map(|y| spans.iter().any(|(a, b)| (*a..*b).contains(&y)))
        .collect()
}

/// The plain card of the hosted reading of run 44267949955: three text lines at a pitch of 14
/// rows, the middle line's box 28.5..42.5 and its room 21.5..49.5.
const PLAIN: [(usize, usize); 3] = [(2, 13), (30, 41), (58, 69)];

#[test]
fn the_runs_of_rows_that_hold_ink() {
    assert_eq!(ink_runs(&rows(72, &PLAIN)), PLAIN.to_vec());
    assert_eq!(ink_runs(&rows(4, &[(2, 4)])), vec![(2, 4)]);
    assert!(ink_runs(&rows(4, &[])).is_empty());
}

#[test]
fn ink_inside_its_room_reads_inside() {
    let plain = rows(72, &PLAIN);
    let r = ink_reading(&plain, &rows(72, &[(2, 13), (30, 48), (58, 69)])).unwrap();
    assert_eq!(r.pitch, 14.0);
    assert_eq!(r.room, (21.5, 49.5));
    assert_eq!(r.ink, (30, 48));
    assert!(r.in_room && !r.reaches, "{r:?}");
    assert_eq!(r.margin(), (8.5, 1.5));
}

#[test]
fn ink_that_leaves_its_room_reads_outside() {
    let plain = rows(72, &PLAIN);
    let below = ink_reading(&plain, &rows(72, &[(2, 13), (30, 50), (58, 69)])).unwrap();
    assert!(
        !below.in_room,
        "ink to row 50 against a room that ends at 49.5: {below:?}"
    );
    assert!(below.margin().1 < 0.0);
    let above = ink_reading(&plain, &rows(72, &[(2, 13), (21, 41), (58, 69)])).unwrap();
    assert!(
        !above.in_room,
        "ink from row 21 against a room that starts at 21.5: {above:?}"
    );
    let next = ink_reading(&plain, &rows(72, &[(2, 13), (30, 57), (58, 69)])).unwrap();
    assert!(
        !next.in_room && next.reaches,
        "ink into the next line's box: {next:?}"
    );
    let control = ink_reading(&plain, &rows(72, &[(0, 13), (14, 41), (58, 69)])).unwrap();
    assert!(control.reaches, "the 64-mark control: {control:?}");
}

#[test]
fn a_plain_card_without_three_lines_or_marked_ink_gives_no_reading() {
    let two = rows(72, &[(2, 13), (58, 69)]);
    assert!(ink_reading(&two, &two).is_err());
    let plain = rows(72, &PLAIN);
    assert!(ink_reading(&plain, &rows(72, &[(2, 13), (58, 69)])).is_err());
}

/// The cases of the hosted target read on this Mac: each card of five lines drawn by AppKit
/// into a bitmap with the alert's small system font, no window opened, and read by the same
/// function. The numbers stand beside the hosted ones; this reads no alert, so it asserts only
/// that every case reads.
#[cfg(target_os = "macos")]
#[test]
fn the_ink_cases_read_on_this_macs_text_path() {
    for (name, plain, marked, kind) in ink::ink_cases() {
        if kind == ink::Ink::Last || kind == ink::Ink::Control {
            continue;
        }
        let card = |middle: &str| {
            format!("Pay to (You gave this): City Power\n\n{middle}\n\nAmount (From the site): 42.10 USD")
        };
        let r = ink_reading(&local::draw(&card(&plain)), &local::draw(&card(&marked)))
            .unwrap_or_else(|why| panic!("{name}: {why}"));
        println!(
            "local ink reading {name} ({kind:?}): pitch={:.1} room={:.1}..{:.1} ink={}..{} margin_above={:.1} margin_below={:.1} in_room={} reaches_another_line={}",
            r.pitch, r.room.0, r.room.1, r.ink.0, r.ink.1, r.margin().0, r.margin().1, r.in_room, r.reaches
        );
    }
}

/// The marks below by count on a letter with a descender, read on this Mac as above: the numbers
/// behind the choice of ruling RD-137 M3.
#[cfg(target_os = "macos")]
#[test]
fn the_marks_below_by_count_read_on_this_macs_text_path() {
    let deep = "Subject (From the site): gy";
    let card = |middle: &str| {
        format!(
            "Pay to (You gave this): City Power\n\n{middle}\n\nAmount (From the site): 42.10 USD"
        )
    };
    for mark in [
        '\u{316}', '\u{323}', '\u{331}', '\u{347}', '\u{353}', '\u{359}', '\u{35c}', '\u{489}',
    ] {
        for count in 1..=3 {
            let marked = format!("{deep}{}", ink::stack(&[mark], count));
            let r = ink_reading(&local::draw(&card(deep)), &local::draw(&card(&marked))).unwrap();
            println!(
                "local below reading U+{:04X}x{count}: room={:.1}..{:.1} ink={}..{} margin_below={:.1} in_room={}",
                u32::from(mark), r.room.0, r.room.1, r.ink.0, r.ink.1, r.margin().1, r.in_room
            );
        }
    }
}

#[cfg(target_os = "macos")]
mod local {
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

    #[link(name = "AppKit", kind = "framework")]
    extern "C" {}

    /// The rows of a bitmap of 520 by 120 pixels that hold ink once `text` is drawn into it with
    /// the small system font.
    pub fn draw(text: &str) -> Vec<bool> {
        let (wide, high) = (520usize, 120usize);
        // SAFETY: each object is made here and lives for the call; the drawing goes to a bitmap
        // that this function owns, through a graphics context saved and restored around it.
        unsafe {
            let text = NSString::from_str(text);
            let size: f64 = msg_send![class!(NSFont), smallSystemFontSize];
            let font: *mut AnyObject = msg_send![class!(NSFont), systemFontOfSize: size];
            let key = NSString::from_str("NSFont");
            let attrs: *mut AnyObject =
                msg_send![class!(NSDictionary), dictionaryWithObject: font, forKey: &*key];
            let string: *mut AnyObject = msg_send![class!(NSAttributedString), alloc];
            let string: *mut AnyObject =
                msg_send![string, initWithString: &*text, attributes: attrs];
            let space = NSString::from_str("NSDeviceRGBColorSpace");
            let rep: *mut AnyObject = msg_send![class!(NSBitmapImageRep), alloc];
            let rep: *mut AnyObject = msg_send![rep, initWithBitmapDataPlanes: std::ptr::null_mut::<*mut u8>(), pixelsWide: wide as isize, pixelsHigh: high as isize, bitsPerSample: 8isize, samplesPerPixel: 4isize, hasAlpha: true, isPlanar: false, colorSpaceName: &*space, bytesPerRow: 0isize, bitsPerPixel: 0isize];
            let context: *mut AnyObject =
                msg_send![class!(NSGraphicsContext), graphicsContextWithBitmapImageRep: rep];
            let _: () = msg_send![class!(NSGraphicsContext), saveGraphicsState];
            let _: () = msg_send![class!(NSGraphicsContext), setCurrentContext: context];
            let rect = NSRect::new(NSPoint::new(10.0, 0.0), NSSize::new(500.0, 110.0));
            let _: () = msg_send![string, drawInRect: rect];
            let _: () = msg_send![context, flushGraphics];
            let _: () = msg_send![class!(NSGraphicsContext), restoreGraphicsState];
            let row: isize = msg_send![rep, bytesPerRow];
            let data: *const u8 = msg_send![rep, bitmapData];
            let row = row as usize;
            let bytes = std::slice::from_raw_parts(data, row * high);
            (0..high)
                .map(|y| (0..wide).any(|x| bytes[y * row + x * 4 + 3] > 48))
                .collect()
        }
    }
}
