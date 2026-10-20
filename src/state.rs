use std::process::Command;
use std::sync::atomic::{AtomicI8, AtomicU8, Ordering};

use crate::constants::FALLBACK_MIN;
use crate::logging::log_d;
use crate::properties::get_prop;
use crate::range::BrightnessRange;

const SOURCE_PROPERTY: u8 = 1;
const SOURCE_DISPLAY_CMD: u8 = 2;
const SOURCE_SETTINGS: u8 = 3;

static FALLBACK_SOURCE: AtomicU8 = AtomicU8::new(0);
static WORKING_ARG_INDEX: AtomicI8 = AtomicI8::new(-1);

fn from_framework_float(value: f32, range: &BrightnessRange) -> i32 {
    if value <= 0.0 {
        return -1;
    }

    if value <= 1.0 {
        let scaled = range.min as f32 + value * (range.max - range.min) as f32;
        scaled.round() as i32
    } else {
        (value.round() as i32).clamp(range.min, range.max)
    }
}

fn property_brightness(range: &BrightnessRange, is_float: bool) -> Option<i32> {
    let raw = get_prop("debug.tracing.screen_brightness")?;
    let raw = raw.trim();

    if raw.is_empty() {
        return None;
    }

    if is_float {
        let value = raw.parse::<f32>().ok()?;
        Some(from_framework_float(value, range))
    } else {
        let value = raw.split('.').next()?.parse::<i32>().ok()?;
        match value {
            0 => Some(-1),
            v => Some(v),
        }
    }
}

fn try_run_display_cmd(args: &[&str], range: &BrightnessRange) -> Option<i32> {
    let output = Command::new("cmd").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        let text = line.trim();
        if let Ok(value) = text.parse::<f32>() {
            return Some(from_framework_float(value, range));
        }
        if let Some(token) = text.split_whitespace().last() {
            if let Ok(value) = token.parse::<f32>() {
                return Some(from_framework_float(value, range));
            }
        }
    }

    None
}

fn display_command_brightness(range: &BrightnessRange) -> Option<i32> {
    const ARG_SETS: [&[&str]; 3] = [
        &["display", "get-brightness", "0"],
        &["display", "get-brightness", "--id", "0"],
        &["display", "get-brightness"],
    ];

    let cached = WORKING_ARG_INDEX.load(Ordering::Relaxed);
    if cached >= 0 && (cached as usize) < ARG_SETS.len() {
        if let Some(val) = try_run_display_cmd(ARG_SETS[cached as usize], range) {
            return Some(val);
        }
    }

    for (idx, args) in ARG_SETS.iter().enumerate() {
        if idx as i8 == cached {
            continue;
        }
        if let Some(val) = try_run_display_cmd(args, range) {
            WORKING_ARG_INDEX.store(idx as i8, Ordering::Relaxed);
            return Some(val);
        }
    }

    None
}

fn settings_brightness(range: &BrightnessRange) -> Option<i32> {
    let output = Command::new("settings")
        .args(["get", "system", "screen_brightness"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let text = stdout.trim();
    let value = text
        .parse::<i32>()
        .ok()
        .or_else(|| text.parse::<f32>().ok().map(|v| v.round() as i32))?;

    match value {
        0 => Some(-1),
        v => Some(v.clamp(range.min, range.max)),
    }
}

fn fallback_brightness(range: &BrightnessRange) -> i32 {
    let source = FALLBACK_SOURCE.load(Ordering::Relaxed);

    if let Some(value) = display_command_brightness(range) {
        if source != SOURCE_DISPLAY_CMD {
            FALLBACK_SOURCE.store(SOURCE_DISPLAY_CMD, Ordering::Relaxed);
            if crate::modes::dbg_on() {
                log_d("[BrightnessSource] Using 'cmd display get-brightness' fallback.");
            }
        }
        return value;
    }

    if let Some(value) = settings_brightness(range) {
        if source != SOURCE_SETTINGS {
            FALLBACK_SOURCE.store(SOURCE_SETTINGS, Ordering::Relaxed);
            if crate::modes::dbg_on() {
                log_d("[BrightnessSource] Using Settings.System screen_brightness fallback.");
            }
        }
        return value;
    }

    FALLBACK_MIN
}

// brightness property getter
pub(crate) fn get_prop_brightness(range: &BrightnessRange, is_float: bool) -> i32 {
    if let Some(value) = property_brightness(range, is_float) {
        FALLBACK_SOURCE.store(SOURCE_PROPERTY, Ordering::Relaxed);
        return value;
    }

    fallback_brightness(range)
}

// screen state getter
pub(crate) fn get_screen_state() -> i32 {
    // screen_state values from debug.tracing.screen_state:
    // 0: OFF
    // 1: OFF (AOD)
    // 2: ON
    // 3: DOZE (AOD)
    // 4: DOZE_SUSPEND (AOD DIMMED)
    get_prop("debug.tracing.screen_state").and_then(|v| v.parse::<i32>().ok()).unwrap_or(2)
}
