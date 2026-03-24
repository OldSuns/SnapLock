use std::process::Command;

#[cfg(all(windows, not(debug_assertions)))]
use std::os::windows::process::CommandExt;

#[cfg(all(windows, not(debug_assertions)))]
const CREATE_NO_WINDOW: u32 = 0x08000000;

/// Applies release-only Windows background process settings so helper tools
/// like ffmpeg do not spawn a visible console window in packaged builds.
#[cfg_attr(not(all(windows, not(debug_assertions))), allow(unused_variables))]
pub fn configure_background_command(command: &mut Command) {
    #[cfg(all(windows, not(debug_assertions)))]
    {
        command.creation_flags(CREATE_NO_WINDOW);
    }
}
