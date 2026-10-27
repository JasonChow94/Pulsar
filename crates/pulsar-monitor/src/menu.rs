use pulsar_core::config::{Config, Display, DisplayMode, LabelStyle, Position};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, MF_CHECKED, MF_GRAYED, MF_SEPARATOR, MF_STRING,
    PostMessageW, SW_SHOWNORMAL, SetForegroundWindow, TPM_BOTTOMALIGN, TPM_NONOTIFY, TPM_RETURNCMD,
    TPM_RIGHTBUTTON, TrackPopupMenu, WM_NULL,
};
use windows::core::{HSTRING, PCWSTR, w};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Mode(DisplayMode),
    Settings,
    Details,
    About,
    TaskManager,
    Exit,
    Toggle(Toggle),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Toggle {
    ShortLabels,
    Icons,
    HoverPopup,
    HideInFullscreen,
    Floating,
    LockPosition,
}

pub fn apply_toggle(c: &mut Config, t: Toggle) {
    let d = &mut c.display;
    match t {
        Toggle::ShortLabels => {
            d.labels = if d.labels == LabelStyle::Short {
                LabelStyle::Full
            } else {
                LabelStyle::Short
            }
        }
        Toggle::Icons => {
            d.icons = !d.icons;
            if !d.icons && d.labels == LabelStyle::None {
                d.labels = LabelStyle::Full;
            }
        }
        Toggle::HoverPopup => d.hover_popup = !d.hover_popup,
        Toggle::HideInFullscreen => d.hide_in_fullscreen = !d.hide_in_fullscreen,
        Toggle::Floating => {
            d.position = if d.position == Position::Floating {
                Position::Right
            } else {
                Position::Floating
            }
        }
        Toggle::LockPosition => d.lock_position = !d.lock_position,
    }
}

const ID_GRAPHS: usize = 1;
const ID_TEXT: usize = 2;
const ID_TASK_MANAGER: usize = 3;
const ID_EXIT: usize = 4;
const ID_SETTINGS: usize = 5;
const ID_ABOUT: usize = 6;
const ID_SHORT_LABELS: usize = 7;
const ID_HOVER: usize = 8;
const ID_HIDE_FULLSCREEN: usize = 9;
const ID_FLOATING: usize = 10;
const ID_LOCK: usize = 11;
const ID_ICONS: usize = 12;
const ID_DETAILS: usize = 13;

pub fn command_for(id: usize) -> Option<Command> {
    match id {
        ID_GRAPHS => Some(Command::Mode(DisplayMode::Graph)),
        ID_TEXT => Some(Command::Mode(DisplayMode::Text)),
        ID_SETTINGS => Some(Command::Settings),
        ID_DETAILS => Some(Command::Details),
        ID_ABOUT => Some(Command::About),
        ID_SHORT_LABELS => Some(Command::Toggle(Toggle::ShortLabels)),
        ID_HOVER => Some(Command::Toggle(Toggle::HoverPopup)),
        ID_HIDE_FULLSCREEN => Some(Command::Toggle(Toggle::HideInFullscreen)),
        ID_FLOATING => Some(Command::Toggle(Toggle::Floating)),
        ID_LOCK => Some(Command::Toggle(Toggle::LockPosition)),
        ID_ICONS => Some(Command::Toggle(Toggle::Icons)),
        ID_TASK_MANAGER => Some(Command::TaskManager),
        ID_EXIT => Some(Command::Exit),
        _ => None,
    }
}

/// Shows the context menu at a screen position and returns the chosen
/// command. Runs a modal loop: the caller must not hold app state borrowed.
pub fn show(owner: HWND, x: i32, y: i32, display: &Display) -> Option<Command> {
    unsafe {
        let menu = CreatePopupMenu().ok()?;
        let check = |on: bool| {
            if on {
                MF_STRING | MF_CHECKED
            } else {
                MF_STRING
            }
        };
        let mode = display.mode;
        let floating = display.position == Position::Floating;
        let _ = AppendMenuW(menu, MF_STRING, ID_DETAILS, w!("系统详细信息…"));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(
            menu,
            check(mode == DisplayMode::Graph),
            ID_GRAPHS,
            w!("图表模式"),
        );
        let _ = AppendMenuW(
            menu,
            check(mode == DisplayMode::Text),
            ID_TEXT,
            w!("仅文本模式"),
        );
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(
            menu,
            check(display.labels == LabelStyle::Short),
            ID_SHORT_LABELS,
            w!("简短标签"),
        );
        let _ = AppendMenuW(menu, check(display.icons), ID_ICONS, w!("图标"));
        let _ = AppendMenuW(
            menu,
            check(display.hover_popup),
            ID_HOVER,
            w!("悬停显示详情"),
        );
        let _ = AppendMenuW(
            menu,
            check(display.hide_in_fullscreen),
            ID_HIDE_FULLSCREEN,
            w!("全屏时隐藏"),
        );
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(menu, check(floating), ID_FLOATING, w!("悬浮"));
        let lock = if floating {
            check(display.lock_position)
        } else {
            check(display.lock_position) | MF_GRAYED
        };
        let _ = AppendMenuW(menu, lock, ID_LOCK, w!("锁定位置"));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(menu, MF_STRING, ID_SETTINGS, w!("设置…"));
        let _ = AppendMenuW(menu, MF_STRING, ID_TASK_MANAGER, w!("打开任务管理器"));
        let _ = AppendMenuW(menu, MF_STRING, ID_ABOUT, w!("关于 Pulsar"));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(menu, MF_STRING, ID_EXIT, w!("退出 Pulsar"));

        // Without being foreground, the menu would not close on an outside click.
        let _ = SetForegroundWindow(owner);
        let flags = TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN;
        let id = TrackPopupMenu(menu, flags, x, y, None, owner, None).0 as usize;
        let _ = PostMessageW(Some(owner), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);
        command_for(id)
    }
}

/// Opens a file, folder or URL with its default handler.
pub fn shell_open(target: &str) {
    unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            &HSTRING::from(target),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        );
    }
}

pub fn open_task_manager() {
    shell_open("taskmgr.exe");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_map_to_commands() {
        assert_eq!(command_for(ID_TEXT), Some(Command::Mode(DisplayMode::Text)));
        assert_eq!(command_for(ID_EXIT), Some(Command::Exit));
        assert_eq!(command_for(ID_SETTINGS), Some(Command::Settings));
        assert_eq!(command_for(ID_ABOUT), Some(Command::About));
        assert_eq!(command_for(ID_DETAILS), Some(Command::Details));
        assert_eq!(command_for(0), None, "menu dismissed");
    }

    #[test]
    fn toggle_ids_map_to_commands() {
        assert_eq!(
            command_for(ID_SHORT_LABELS),
            Some(Command::Toggle(Toggle::ShortLabels))
        );
        assert_eq!(
            command_for(ID_FLOATING),
            Some(Command::Toggle(Toggle::Floating))
        );
        assert_eq!(
            command_for(ID_LOCK),
            Some(Command::Toggle(Toggle::LockPosition))
        );
    }

    #[test]
    fn toggles_flip_the_config() {
        use pulsar_core::config::{Config, LabelStyle, Position};
        let mut c = Config::default();
        apply_toggle(&mut c, Toggle::Floating);
        assert_eq!(c.display.position, Position::Floating);
        apply_toggle(&mut c, Toggle::Floating);
        assert_eq!(c.display.position, Position::Right);
        apply_toggle(&mut c, Toggle::HideInFullscreen);
        assert!(!c.display.hide_in_fullscreen);
        apply_toggle(&mut c, Toggle::ShortLabels);
        assert_eq!(c.display.labels, LabelStyle::Short);
        apply_toggle(&mut c, Toggle::ShortLabels);
        assert_eq!(c.display.labels, LabelStyle::Full);
    }

    #[test]
    fn icon_toggle_restores_labels() {
        use pulsar_core::config::{Config, LabelStyle};
        let mut c = Config::default();
        apply_toggle(&mut c, Toggle::Icons);
        assert!(c.display.icons);
        c.display.labels = LabelStyle::None;
        apply_toggle(&mut c, Toggle::Icons);
        assert!(!c.display.icons);
        assert_eq!(
            c.display.labels,
            LabelStyle::Full,
            "a cell never ends up with no label at all"
        );
        assert_eq!(command_for(ID_ICONS), Some(Command::Toggle(Toggle::Icons)));
    }
}
