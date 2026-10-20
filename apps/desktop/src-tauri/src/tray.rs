//! System tray icon and menu.

use tauri::menu::{Menu, MenuEvent, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Runtime};

use crate::{main_window, settings_window};

pub const TRAY_ID: &str = "main";

/// Actions triggered from the tray menu, keyed by menu item id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrayAction {
    OpenSettings,
    OpenMainWindow,
    Quit,
}

impl TrayAction {
    /// In menu order (D047).
    const ALL: [Self; 3] = [Self::OpenSettings, Self::OpenMainWindow, Self::Quit];

    fn id(self) -> &'static str {
        match self {
            Self::OpenSettings => "open_settings",
            Self::OpenMainWindow => "open_main_window",
            Self::Quit => "quit",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::OpenSettings => "设置…",
            Self::OpenMainWindow => "打开主窗口",
            Self::Quit => "退出 LexTrail",
        }
    }

    fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|action| action.id() == id)
    }
}

pub fn create<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let items = TrayAction::ALL
        .into_iter()
        .map(|action| MenuItem::with_id(app, action.id(), action.label(), true, None::<&str>))
        .collect::<tauri::Result<Vec<_>>>()?;
    let item_refs: Vec<&dyn tauri::menu::IsMenuItem<R>> =
        items.iter().map(|item| item as _).collect();
    let menu = Menu::with_items(app, &item_refs)?;

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("LexTrail")
        .menu(&menu)
        // Left click opens the main window; the menu is on right click only.
        .show_menu_on_left_click(false)
        .on_menu_event(handle_menu_event)
        .on_tray_icon_event(handle_icon_event);
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

fn handle_menu_event<R: Runtime>(app: &AppHandle<R>, event: MenuEvent) {
    match TrayAction::from_id(event.id().as_ref()) {
        Some(TrayAction::OpenSettings) => settings_window::open(app),
        Some(TrayAction::OpenMainWindow) => main_window::open(app),
        Some(TrayAction::Quit) => app.exit(0),
        None => {}
    }
}

fn handle_icon_event<R: Runtime>(tray: &TrayIcon<R>, event: TrayIconEvent) {
    if let TrayIconEvent::Click {
        button: MouseButton::Left,
        button_state: MouseButtonState::Up,
        ..
    } = event
    {
        main_window::open(tray.app_handle());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_ids_round_trip() {
        for action in TrayAction::ALL {
            assert_eq!(TrayAction::from_id(action.id()), Some(action));
        }
    }

    #[test]
    fn menu_ids_are_unique() {
        let mut ids: Vec<_> = TrayAction::ALL.iter().map(|a| a.id()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), TrayAction::ALL.len());
    }

    #[test]
    fn menu_order_is_settings_main_window_quit() {
        let labels: Vec<_> = TrayAction::ALL.iter().map(|a| a.label()).collect();
        assert_eq!(labels, ["设置…", "打开主窗口", "退出 LexTrail"]);
    }

    #[test]
    fn unknown_id_is_ignored() {
        assert_eq!(TrayAction::from_id("nope"), None);
    }
}
