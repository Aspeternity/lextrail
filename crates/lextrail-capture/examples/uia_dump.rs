//! Diagnostic: after a delay, dumps the raw UI Automation data for the foreground window's
//! selection: each selection range's text, its enclosing paragraph and line, the prefix
//! length LexTrail derives the offset from, and the text under the range's bounding box.
//! Used to tell a LexTrail bug from a provider bug (e.g. Edge's PDF viewer).
//!
//! ```text
//! cargo run -p lextrail-capture --example uia_dump -- [delay_secs]
//! ```

#[cfg(windows)]
fn main() -> windows::core::Result<()> {
    use std::time::Duration;

    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
    };
    use windows::Win32::UI::Accessibility::{
        CUIAutomation8, IUIAutomation, IUIAutomationTextPattern, IUIAutomationTextRange,
        TextPatternRangeEndpoint_End, TextPatternRangeEndpoint_Start, TextUnit_Line,
        TextUnit_Paragraph, UIA_TextPatternId,
    };

    let delay = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(3);
    eprintln!("dumping in {delay} s: switch to a window and select some text");
    std::thread::sleep(Duration::from_secs(delay));

    let text = |r: &IUIAutomationTextRange| -> String {
        // SAFETY: COM call on a valid range.
        unsafe { r.GetText(2000) }
            .map(|b| String::from_utf16_lossy(&b))
            .unwrap_or_else(|e| format!("<error {e}>"))
    };

    // SAFETY: plain COM setup and calls on this thread.
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
        let automation: IUIAutomation =
            CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER)?;
        let walker = automation.ControlViewWalker()?;
        let mut element = automation.GetFocusedElement()?;
        println!(
            "focused: {:?} ({:?})",
            element.CurrentName().map(|n| n.to_string()),
            element.CurrentLocalizedControlType().map(|n| n.to_string())
        );
        let pattern = loop {
            if let Ok(p) =
                element.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId)
            {
                break p;
            }
            element = walker.GetParentElement(&element)?;
        };
        println!(
            "text pattern on: {:?} ({:?})",
            element.CurrentName().map(|n| n.to_string()),
            element.CurrentLocalizedControlType().map(|n| n.to_string())
        );

        // The element with the text pattern and its ancestors, to see what identifies it.
        let mut node = Some(element.clone());
        let mut depth = 0;
        while let Some(e) = node {
            println!(
                "  ancestor {depth}: type={:?} class={:?} automation_id={:?} framework={:?} name={:?}",
                e.CurrentLocalizedControlType().map(|n| n.to_string()),
                e.CurrentClassName().map(|n| n.to_string()),
                e.CurrentAutomationId().map(|n| n.to_string()),
                e.CurrentFrameworkId().map(|n| n.to_string()),
                e.CurrentName()
                    .map(|n| n.to_string().chars().take(60).collect::<String>()),
            );
            depth += 1;
            node = walker.GetParentElement(&e).ok().filter(|_| depth < 12);
        }

        let ranges = pattern.GetSelection()?;
        println!("selection ranges: {}", ranges.Length()?);
        for i in 0..ranges.Length()? {
            let range = ranges.GetElement(i)?;
            let selected = text(&range);
            println!("\n[{i}] selection GetText: {selected:?}");
            for (name, unit) in [("paragraph", TextUnit_Paragraph), ("line", TextUnit_Line)] {
                let enclosing = range.Clone()?;
                enclosing.ExpandToEnclosingUnit(unit)?;
                let whole = text(&enclosing);
                let prefix = enclosing.Clone()?;
                prefix.MoveEndpointByRange(
                    TextPatternRangeEndpoint_End,
                    &range,
                    TextPatternRangeEndpoint_Start,
                )?;
                let prefix_text = text(&prefix);
                let start = prefix_text.encode_utf16().count();
                let units: Vec<u16> = whole.encode_utf16().collect();
                let len = selected.encode_utf16().count();
                let at_offset = units
                    .get(start..(start + len).min(units.len()))
                    .map(String::from_utf16_lossy);
                println!("  {name}: {whole:?}");
                println!("  {name} prefix length (UTF-16): {start}");
                println!("  {name} text at that offset: {at_offset:?}");
                println!(
                    "  {name} contains selection text at: {:?}",
                    whole.find(&selected)
                );
            }
            // What is actually on screen where the range says it is.
            let rects = range.GetBoundingRectangles()?;
            let n = (*rects).rgsabound[0].cElements as usize;
            let data = (*rects).pvData as *const f64;
            let values: Vec<f64> = (0..n).map(|k| *data.add(k)).collect();
            println!("  bounding rectangles (x, y, w, h)*: {values:?}");
            if values.len() >= 4 {
                let point = windows::Win32::Foundation::POINT {
                    x: (values[0] + values[2] / 2.0) as i32,
                    y: (values[1] + values[3] / 2.0) as i32,
                };
                if let Ok(under) = pattern.RangeFromPoint(point) {
                    under
                        .ExpandToEnclosingUnit(windows::Win32::UI::Accessibility::TextUnit_Word)?;
                    println!(
                        "  word under the range's first rectangle: {:?}",
                        text(&under)
                    );
                }
            }
        }
    }
    Ok(())
}

#[cfg(not(windows))]
fn main() {
    eprintln!("uia_dump only runs on Windows");
}
