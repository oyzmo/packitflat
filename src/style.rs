// SPDX-License-Identifier: GPL-3.0-or-later

//! The app's colour palette, applied over libadwaita's defaults.
//!
//! Sage / cream / cocoa / tan, the same palette as the sibling apps. Both
//! spellings of the override are emitted: libadwaita >= 1.6 reads the `--name`
//! custom properties, older versions the `@define-color` names. The stylesheet
//! is rebuilt whenever the system switches between light and dark, because this
//! app follows the system scheme rather than forcing one.

use gtk::gdk;

/// Sage / cream / cocoa / tan on a cream ground.
const LIGHT: &str = "
:root {
    --accent-color: #4F6B4B;
    --accent-bg-color: #7FA07A;
    --accent-fg-color: #4A3428;
    --window-bg-color: #FBF5EA;
    --window-fg-color: #4A3428;
    --view-bg-color: #FBF5EA;
    --view-fg-color: #4A3428;
    --headerbar-bg-color: #F4E7D3;
    --headerbar-fg-color: #4A3428;
    --headerbar-backdrop-color: #F4E7D3;
    --card-bg-color: #F4E7D3;
    --card-fg-color: #4A3428;
    --popover-bg-color: #FBF5EA;
    --popover-fg-color: #4A3428;
    --dialog-bg-color: #FBF5EA;
    --dialog-fg-color: #4A3428;
    --sidebar-bg-color: #F4E7D3;
    --sidebar-fg-color: #4A3428;
    --borders: rgba(138, 106, 80, 0.40);
}
@define-color accent_color #4F6B4B;
@define-color accent_bg_color #7FA07A;
@define-color accent_fg_color #4A3428;
@define-color window_bg_color #FBF5EA;
@define-color window_fg_color #4A3428;
@define-color view_bg_color #FBF5EA;
@define-color view_fg_color #4A3428;
@define-color headerbar_bg_color #F4E7D3;
@define-color headerbar_fg_color #4A3428;
@define-color headerbar_backdrop_color #F4E7D3;
@define-color card_bg_color #F4E7D3;
@define-color card_fg_color #4A3428;
@define-color popover_bg_color #FBF5EA;
@define-color popover_fg_color #4A3428;
@define-color dialog_bg_color #FBF5EA;
@define-color dialog_fg_color #4A3428;
@define-color sidebar_bg_color #F4E7D3;
@define-color sidebar_fg_color #4A3428;
@define-color borders rgba(138, 106, 80, 0.40);
";

/// Surfaces swapped: cream text on cocoa, with a lightened sage accent so it
/// still clears WCAG AA on the dark ground.
const DARK: &str = "
:root {
    --accent-color: #9DBE98;
    --accent-bg-color: #7FA07A;
    --accent-fg-color: #2E2019;
    --window-bg-color: #2E2019;
    --window-fg-color: #F4E7D3;
    --view-bg-color: #251A14;
    --view-fg-color: #F4E7D3;
    --headerbar-bg-color: #3A281F;
    --headerbar-fg-color: #F4E7D3;
    --headerbar-backdrop-color: #2E2019;
    --card-bg-color: #3A281F;
    --card-fg-color: #F4E7D3;
    --popover-bg-color: #3A281F;
    --popover-fg-color: #F4E7D3;
    --dialog-bg-color: #3A281F;
    --dialog-fg-color: #F4E7D3;
    --sidebar-bg-color: #3A281F;
    --sidebar-fg-color: #F4E7D3;
    --borders: rgba(138, 106, 80, 0.55);
}
@define-color accent_color #9DBE98;
@define-color accent_bg_color #7FA07A;
@define-color accent_fg_color #2E2019;
@define-color window_bg_color #2E2019;
@define-color window_fg_color #F4E7D3;
@define-color view_bg_color #251A14;
@define-color view_fg_color #F4E7D3;
@define-color headerbar_bg_color #3A281F;
@define-color headerbar_fg_color #F4E7D3;
@define-color headerbar_backdrop_color #2E2019;
@define-color card_bg_color #3A281F;
@define-color card_fg_color #F4E7D3;
@define-color popover_bg_color #3A281F;
@define-color popover_fg_color #F4E7D3;
@define-color dialog_bg_color #3A281F;
@define-color dialog_fg_color #F4E7D3;
@define-color sidebar_bg_color #3A281F;
@define-color sidebar_fg_color #F4E7D3;
@define-color borders rgba(138, 106, 80, 0.55);
";

/// The app's own rules. Deliberately tiny: an import note has to read as a
/// warning at a glance, and everything else is stock Adwaita.
const SHARED: &str = "
.note-warning image {
    color: #B4762A;
}
.note-info image {
    color: @accent_color;
}

/* The example inside an empty box has to look like an example. At Adwaita's
   default weight people read “no.oyzmo.PackItFlat” as an answer already given
   and press Next, which is exactly the confusion this app exists to remove. */
text > placeholder {
    opacity: 0.42;
    font-style: italic;
}
";

/// Load the palette for the current scheme and keep it in step with it.
pub fn install() {
    let Some(display) = gdk::Display::default() else {
        return;
    };
    let provider = gtk::CssProvider::new();
    gtk::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    let manager = adw::StyleManager::default();
    let load = {
        let provider = provider.clone();
        move |dark: bool| {
            let palette = if dark { DARK } else { LIGHT };
            provider.load_from_string(&format!("{palette}{SHARED}"));
        }
    };
    load(manager.is_dark());
    manager.connect_dark_notify(move |manager| load(manager.is_dark()));
}
