//! System tray icon and menu.

use tauri::menu::{Menu, MenuEvent, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Runtime};

const TRAY_ID: &str = "main";

/// Actions triggered from the tray menu, keyed by menu item id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrayAction {
    Quit,
}

impl TrayAction {
    const ALL: [Self; 1] = [Self::Quit];

    fn id(self) -> &'static str {
        match self {
            Self::Quit => "quit",
        }
    }

    fn label(self) -> &'static str {
        match self {
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
        .on_menu_event(handle_menu_event);
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

fn handle_menu_event<R: Runtime>(app: &AppHandle<R>, event: MenuEvent) {
    match TrayAction::from_id(event.id().as_ref()) {
        Some(TrayAction::Quit) => app.exit(0),
        None => {}
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
    fn unknown_id_is_ignored() {
        assert_eq!(TrayAction::from_id("nope"), None);
    }
}
