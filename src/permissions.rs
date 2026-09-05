//! What the finished app is allowed to do, in words rather than in flags.
//!
//! This is the one part of the app where being wrong is dangerous rather than
//! annoying: a manifest that quietly asks for the whole home directory is a
//! manifest whose users have no idea what they agreed to. So three rules hold
//! here:
//!
//! 1. **Nothing is lost.** Flags this app doesn't model are kept verbatim and
//!    written back, the same as everywhere else in the model.
//! 2. **Every switch says what it costs.** Each one carries what it allows and
//!    what stops working without it, because "turn this off to be safe" is
//!    useless advice if the app then won't start.
//! 3. **The risk summary describes the app, not the flags.** "Can read and
//!    change every file in your home folder" — not "--filesystem=home".

use serde_yaml_ng::Mapping;

/// One switch the user can flip, and everything the UI needs to explain it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Wayland,
    FallbackX11,
    X11,
    Ipc,
    Audio,
    Network,
    Gpu,
    AllDevices,
}

impl Key {
    pub fn label(&self) -> &'static str {
        match self {
            Key::Wayland => "Show a window",
            Key::FallbackX11 => "Work on older desktops",
            Key::X11 => "Use the old display system directly",
            Key::Ipc => "Share memory with the display server",
            Key::Audio => "Play and record sound",
            Key::Network => "Use the internet",
            Key::Gpu => "Use the graphics card",
            Key::AllDevices => "Use every device",
        }
    }

    /// What it lets the app do.
    pub fn explanation(&self) -> &'static str {
        match self {
            Key::Wayland => {
                "Lets the app open a window on modern Linux desktops. Almost every app \
                 needs this."
            }
            Key::FallbackX11 => {
                "Lets the app open a window on desktops that still use X11, but only when \
                 the modern way isn't available."
            }
            Key::X11 => {
                "Uses X11 always, even where the modern display system is available. Under \
                 X11 any running app can read what you type in any other."
            }
            Key::Ipc => {
                "Lets the app pass images to the display server without copying them. \
                 Standard for anything that draws."
            }
            Key::Audio => "Lets the app play sound, and record from the microphone.",
            Key::Network => {
                "Lets the app connect to the internet and to other computers on your network."
            }
            Key::Gpu => "Lets the app use the graphics card for drawing and video.",
            Key::AllDevices => {
                "Lets the app use every device on the computer: webcams, microphones, \
                 game controllers, anything plugged in."
            }
        }
    }

    /// What stops working if it is switched off. The brief's rule: never ask
    /// someone to give something up without saying what it costs.
    pub fn consequence(&self) -> &'static str {
        match self {
            Key::Wayland => "The app cannot show a window at all on a modern desktop.",
            Key::FallbackX11 => {
                "The app won't start on older desktops, or in a remote session that uses X11."
            }
            Key::X11 => "Nothing, unless the app specifically needs old X11 features.",
            Key::Ipc => "Drawing may be slower, and some toolkits complain.",
            Key::Audio => "The app is silent, and cannot record.",
            Key::Network => {
                "The app cannot reach anything online: no updates, no downloads, no sign-in."
            }
            Key::Gpu => "Drawing falls back to the processor. Video and games become slow.",
            Key::AllDevices => "The app cannot reach webcams, controllers or other hardware.",
        }
    }

    fn flag(&self) -> &'static str {
        match self {
            Key::Wayland => "--socket=wayland",
            Key::FallbackX11 => "--socket=fallback-x11",
            Key::X11 => "--socket=x11",
            Key::Ipc => "--share=ipc",
            Key::Audio => "--socket=pulseaudio",
            Key::Network => "--share=network",
            Key::Gpu => "--device=dri",
            Key::AllDevices => "--device=all",
        }
    }
}

/// A group of switches, as the step shows them.
pub struct Group {
    pub title: &'static str,
    pub description: &'static str,
    pub keys: &'static [Key],
}

pub const GROUPS: &[Group] = &[
    Group {
        title: "The screen",
        description: "How the app draws its window. Every app with a window needs the \
                      first two of these.",
        keys: &[Key::Wayland, Key::FallbackX11, Key::X11, Key::Ipc],
    },
    Group {
        title: "Hardware",
        description: "Parts of the computer the app can reach.",
        keys: &[Key::Gpu, Key::Audio, Key::AllDevices],
    },
    Group {
        title: "The outside world",
        description: "Whether the app can talk to anything beyond this computer.",
        keys: &[Key::Network],
    },
];

/// Places an app can be given access to, with what each one means. The order is
/// least dangerous first, which is also the order the picker shows.
pub const FILESYSTEM_PRESETS: &[(&str, &str, &str)] = &[
    (
        "xdg-documents",
        "Documents",
        "Everything in the Documents folder.",
    ),
    (
        "xdg-download",
        "Downloads",
        "Everything in the Downloads folder.",
    ),
    (
        "xdg-pictures",
        "Pictures",
        "Everything in the Pictures folder.",
    ),
    ("xdg-music", "Music", "Everything in the Music folder."),
    ("xdg-videos", "Videos", "Everything in the Videos folder."),
    (
        "home",
        "The whole home folder",
        "Every file belonging to the person using the app, including things it has \
         nothing to do with.",
    ),
    (
        "host",
        "Every file on the computer",
        "The entire file system, including system files. There is almost never a good \
         reason for this.",
    ),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    /// Worth knowing about, not worth worrying about.
    Low,
    /// Most people would want to be asked about this.
    Notable,
    /// This is the sandbox mostly switched off.
    Serious,
}

#[derive(Debug, Clone)]
pub struct Risk {
    pub level: Level,
    /// What the app can do, said plainly and in the present tense.
    pub headline: String,
    /// Why that matters, or when it is reasonable.
    pub detail: String,
    /// The way to get the same result without the permission, when there is one.
    pub instead: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Permissions {
    pub wayland: bool,
    pub fallback_x11: bool,
    pub x11: bool,
    pub ipc: bool,
    pub audio: bool,
    pub network: bool,
    pub gpu: bool,
    pub all_devices: bool,
    /// The values after `--filesystem=`, exactly as written (`home`, `host:ro`,
    /// `~/Games`, …).
    pub filesystems: Vec<String>,
    pub talk_names: Vec<String>,
    pub system_talk_names: Vec<String>,
    pub own_names: Vec<String>,
    pub env: Vec<(String, String)>,
    /// Anything this app doesn't model, kept exactly as written.
    pub other: Vec<String>,
}

impl Permissions {
    /// Read `finish-args` as they stand in a manifest.
    pub fn parse(args: &[String]) -> Self {
        let mut permissions = Permissions::default();

        for arg in args {
            let arg = arg.trim();
            if arg.is_empty() {
                continue;
            }
            match arg {
                "--socket=wayland" => permissions.wayland = true,
                "--socket=fallback-x11" => permissions.fallback_x11 = true,
                "--socket=x11" => permissions.x11 = true,
                "--share=ipc" => permissions.ipc = true,
                "--socket=pulseaudio" => permissions.audio = true,
                "--share=network" => permissions.network = true,
                "--device=dri" => permissions.gpu = true,
                "--device=all" => permissions.all_devices = true,
                _ => {
                    if let Some(value) = arg.strip_prefix("--filesystem=") {
                        permissions.filesystems.push(value.to_string());
                    } else if let Some(value) = arg.strip_prefix("--talk-name=") {
                        permissions.talk_names.push(value.to_string());
                    } else if let Some(value) = arg.strip_prefix("--system-talk-name=") {
                        permissions.system_talk_names.push(value.to_string());
                    } else if let Some(value) = arg.strip_prefix("--own-name=") {
                        permissions.own_names.push(value.to_string());
                    } else if let Some(value) = arg.strip_prefix("--env=") {
                        match value.split_once('=') {
                            Some((name, content)) => permissions
                                .env
                                .push((name.to_string(), content.to_string())),
                            None => permissions.other.push(arg.to_string()),
                        }
                    } else {
                        permissions.other.push(arg.to_string());
                    }
                }
            }
        }

        permissions
    }

    /// Back to `finish-args`, in a settled order so that flipping a switch on and
    /// off again doesn't reshuffle somebody's file.
    pub fn to_args(&self) -> Vec<String> {
        let mut args = Vec::new();

        for (on, key) in [
            (self.wayland, Key::Wayland),
            (self.fallback_x11, Key::FallbackX11),
            (self.x11, Key::X11),
            (self.ipc, Key::Ipc),
            (self.gpu, Key::Gpu),
            (self.audio, Key::Audio),
            (self.all_devices, Key::AllDevices),
            (self.network, Key::Network),
        ] {
            if on {
                args.push(key.flag().to_string());
            }
        }

        args.extend(
            self.filesystems
                .iter()
                .map(|value| format!("--filesystem={value}")),
        );
        args.extend(
            self.talk_names
                .iter()
                .map(|name| format!("--talk-name={name}")),
        );
        args.extend(
            self.system_talk_names
                .iter()
                .map(|name| format!("--system-talk-name={name}")),
        );
        args.extend(self.own_names.iter().map(|name| format!("--own-name={name}")));
        args.extend(
            self.env
                .iter()
                .map(|(name, value)| format!("--env={name}={value}")),
        );
        args.extend(self.other.iter().cloned());
        args
    }

    pub fn get(&self, key: Key) -> bool {
        match key {
            Key::Wayland => self.wayland,
            Key::FallbackX11 => self.fallback_x11,
            Key::X11 => self.x11,
            Key::Ipc => self.ipc,
            Key::Audio => self.audio,
            Key::Network => self.network,
            Key::Gpu => self.gpu,
            Key::AllDevices => self.all_devices,
        }
    }

    pub fn set(&mut self, key: Key, on: bool) {
        let slot = match key {
            Key::Wayland => &mut self.wayland,
            Key::FallbackX11 => &mut self.fallback_x11,
            Key::X11 => &mut self.x11,
            Key::Ipc => &mut self.ipc,
            Key::Audio => &mut self.audio,
            Key::Network => &mut self.network,
            Key::Gpu => &mut self.gpu,
            Key::AllDevices => &mut self.all_devices,
        };
        *slot = on;
    }

    pub fn add_filesystem(&mut self, value: &str) {
        let value = value.trim();
        if !value.is_empty() && !self.filesystems.iter().any(|existing| existing == value) {
            self.filesystems.push(value.to_string());
        }
    }

    pub fn remove_filesystem(&mut self, index: usize) {
        if index < self.filesystems.len() {
            self.filesystems.remove(index);
        }
    }

    /// Everything this app would be able to do, worst first. This is what the
    /// step shows above the switches, and what someone reads before deciding
    /// whether they trust it.
    pub fn risks(&self) -> Vec<Risk> {
        let mut risks = Vec::new();

        for value in &self.filesystems {
            let (path, mode) = split_filesystem(value);
            let writable = mode != "ro";
            let verb = if writable {
                "read and change"
            } else {
                "read"
            };

            match path {
                "host" | "host-os" | "host-etc" => risks.push(Risk {
                    level: Level::Serious,
                    headline: format!("This app can {verb} every file on the computer."),
                    detail: "That includes system files and every other user's documents. \
                             Almost no app needs this, and app stores ask why."
                        .into(),
                    instead: Some(portal_hint()),
                }),
                "home" => risks.push(Risk {
                    level: Level::Serious,
                    headline: format!(
                        "This app can {verb} everything in your home folder."
                    ),
                    detail: "Not just its own files: photos, tax returns, saved passwords \
                             in other apps' folders — all of it."
                        .into(),
                    instead: Some(portal_hint()),
                }),
                other if other.starts_with("xdg-") => risks.push(Risk {
                    level: Level::Notable,
                    headline: format!("This app can {verb} your {} folder.", friendly_xdg(other)),
                    detail: "Reasonable for an app that works with those files all the \
                             time, and heavy-handed for one that opens a file now and then."
                        .into(),
                    instead: Some(portal_hint()),
                }),
                other => risks.push(Risk {
                    level: Level::Low,
                    headline: format!("This app can {verb} {other}."),
                    detail: "A specific place, which is the way to do it if the app really \
                             does need files without asking."
                        .into(),
                    instead: None,
                }),
            }
        }

        if self
            .talk_names
            .iter()
            .any(|name| name == "org.freedesktop.Flatpak")
        {
            risks.push(Risk {
                level: Level::Serious,
                headline: "This app can run programs outside the sandbox.".into(),
                detail: "It can start anything on the computer with your account's full \
                         rights, which means the sandbox no longer protects anyone. Only \
                         development tools have a good reason for this — and they have to \
                         say so in their description."
                    .into(),
                instead: None,
            });
        }

        for name in self
            .talk_names
            .iter()
            .chain(self.system_talk_names.iter())
            .filter(|name| name.contains('*') && name.as_str() != "org.freedesktop.Flatpak")
        {
            risks.push(Risk {
                level: Level::Notable,
                headline: format!("This app can talk to any service matching “{name}”."),
                detail: "A wildcard covers services that don't exist yet, so nobody can \
                         say what this will allow next year. Name the services instead."
                    .into(),
                instead: None,
            });
        }

        if self.all_devices {
            risks.push(Risk {
                level: Level::Notable,
                headline: "This app can use every device, including webcams and microphones."
                    .into(),
                detail: "Fine for something that talks to hardware; heavy-handed otherwise."
                    .into(),
                instead: Some(
                    "If it only needs the camera, the camera portal asks the person once \
                     and needs no permission here."
                        .into(),
                ),
            });
        }

        if self.network {
            risks.push(Risk {
                level: Level::Notable,
                headline: "This app can connect to the internet.".into(),
                detail: "Expected for anything that downloads, syncs or signs in. Worth \
                         removing from an app that works entirely offline."
                    .into(),
                instead: None,
            });
        }

        if self.x11 {
            risks.push(Risk {
                level: Level::Notable,
                headline: "This app uses the old display system, where apps can watch each \
                           other."
                    .into(),
                detail: "Under X11 any app can read what you type into any other. Modern \
                         desktops don't work that way."
                    .into(),
                instead: Some(
                    "Switch on “Show a window” and “Work on older desktops” instead: the \
                     app then uses the modern system where it exists, and X11 only where \
                     it must."
                        .into(),
                ),
            });
        }

        if !self.wayland && !self.x11 && !self.fallback_x11 {
            risks.push(Risk {
                level: Level::Notable,
                headline: "This app cannot show a window at all.".into(),
                detail: "Right for a command-line tool, and a mistake for anything else."
                    .into(),
                instead: None,
            });
        }

        // Worst first: the thing someone most needs to know goes at the top.
        risks.sort_by_key(|risk| std::cmp::Reverse(risk.level));
        risks
    }

    /// One line for the top of the step: how exposed this app is overall.
    pub fn summary(&self) -> String {
        let risks = self.risks();
        match risks.first().map(|risk| risk.level) {
            Some(Level::Serious) => {
                "This app asks for a lot. Anyone installing it is trusting you with their \
                 files."
                    .into()
            }
            Some(Level::Notable) => {
                "This app asks for a few things beyond drawing its window.".into()
            }
            Some(Level::Low) | None => {
                "This app keeps to itself: it can draw a window and little else.".into()
            }
        }
    }

    /// The environment as the manifest's `--env=` flags see it, for the editor's
    /// form.
    pub fn env_mapping(&self) -> Mapping {
        let mut mapping = Mapping::new();
        for (name, value) in &self.env {
            mapping.insert(name.as_str().into(), value.as_str().into());
        }
        mapping
    }
}

fn portal_hint() -> String {
    "A file chooser needs no permission at all: when the person picks a file, the app is \
     given that file and nothing else. Use the file chooser portal unless the app really \
     must open files nobody chose."
        .to_string()
}

fn split_filesystem(value: &str) -> (&str, &str) {
    match value.rsplit_once(':') {
        Some((path, mode)) if matches!(mode, "ro" | "rw" | "create") => (path, mode),
        _ => (value, "rw"),
    }
}

fn friendly_xdg(value: &str) -> &str {
    match value {
        "xdg-documents" => "Documents",
        "xdg-download" => "Downloads",
        "xdg-pictures" => "Pictures",
        "xdg-music" => "Music",
        "xdg-videos" => "Videos",
        "xdg-desktop" => "Desktop",
        "xdg-config" => "settings",
        "xdg-cache" => "cache",
        "xdg-data" => "application data",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_usual_set_reads_back_as_switches() {
        let permissions = Permissions::parse(&args(&[
            "--socket=wayland",
            "--socket=fallback-x11",
            "--share=ipc",
            "--device=dri",
        ]));

        assert!(permissions.wayland && permissions.fallback_x11);
        assert!(permissions.ipc && permissions.gpu);
        assert!(!permissions.network && !permissions.all_devices);
        assert!(permissions.filesystems.is_empty());
        assert!(permissions.other.is_empty());
    }

    #[test]
    fn everything_survives_a_round_trip() {
        let original = args(&[
            "--socket=wayland",
            "--share=network",
            "--filesystem=home",
            "--filesystem=~/Games:ro",
            "--talk-name=org.freedesktop.Flatpak",
            "--system-talk-name=org.freedesktop.UDisks2",
            "--own-name=no.oyzmo.Thing",
            "--env=CARGO_HOME=/run/build/cargo",
            "--persist=.thing",
            "--unshare=network",
        ]);

        let permissions = Permissions::parse(&original);
        let written = permissions.to_args();

        for arg in &original {
            assert!(written.contains(arg), "{arg} was lost");
        }
        assert_eq!(written.len(), original.len());
        // …and reading them again gives exactly the same thing.
        assert_eq!(Permissions::parse(&written), permissions);
    }

    #[test]
    fn flags_the_app_does_not_model_are_kept_verbatim() {
        let permissions = Permissions::parse(&args(&["--persist=.config/thing", "--nosuchflag"]));
        assert_eq!(permissions.other.len(), 2);
        assert!(permissions.to_args().contains(&"--nosuchflag".to_string()));
    }

    #[test]
    fn switching_something_on_and_off_leaves_the_list_as_it_was() {
        let original = args(&["--socket=wayland", "--device=dri", "--persist=.thing"]);
        let mut permissions = Permissions::parse(&original);

        permissions.set(Key::Network, true);
        permissions.set(Key::Network, false);
        assert_eq!(permissions.to_args(), original);
    }

    #[test]
    fn the_home_folder_is_described_as_what_it_is() {
        let permissions = Permissions::parse(&args(&["--socket=wayland", "--filesystem=home"]));
        let risks = permissions.risks();

        assert_eq!(risks[0].level, Level::Serious);
        assert!(risks[0].headline.contains("everything in your home folder"));
        assert!(risks[0].instead.as_ref().unwrap().contains("file chooser"));
        assert!(permissions.summary().contains("asks for a lot"));
    }

    #[test]
    fn read_only_access_is_described_as_read_only() {
        let permissions = Permissions::parse(&args(&["--filesystem=host:ro"]));
        let risks = permissions.risks();
        assert!(risks[0].headline.starts_with("This app can read every file"));
        assert!(!risks[0].headline.contains("change"));
    }

    #[test]
    fn talking_to_flatpak_itself_is_called_what_it_is() {
        let permissions = Permissions::parse(&args(&[
            "--socket=wayland",
            "--talk-name=org.freedesktop.Flatpak",
        ]));
        let risks = permissions.risks();
        assert_eq!(risks[0].level, Level::Serious);
        assert!(risks[0].headline.contains("outside the sandbox"));
    }

    #[test]
    fn wildcards_in_service_names_are_flagged() {
        let permissions = Permissions::parse(&args(&["--talk-name=org.gnome.*"]));
        assert!(permissions
            .risks()
            .iter()
            .any(|risk| risk.headline.contains("org.gnome.*")));
    }

    #[test]
    fn plain_x11_gets_the_modern_suggestion() {
        let permissions = Permissions::parse(&args(&["--socket=x11"]));
        let risk = permissions
            .risks()
            .into_iter()
            .find(|risk| risk.headline.contains("old display system"))
            .expect("x11 is flagged");
        assert!(risk.instead.unwrap().contains("Work on older desktops"));
    }

    #[test]
    fn an_app_with_no_window_is_pointed_out() {
        let permissions = Permissions::parse(&args(&["--share=network"]));
        assert!(permissions
            .risks()
            .iter()
            .any(|risk| risk.headline.contains("cannot show a window")));
    }

    #[test]
    fn a_modest_app_is_described_as_modest() {
        let permissions = Permissions::parse(&args(&[
            "--socket=wayland",
            "--socket=fallback-x11",
            "--share=ipc",
            "--device=dri",
        ]));
        assert!(permissions.risks().is_empty());
        assert!(permissions.summary().contains("keeps to itself"));
    }

    #[test]
    fn every_switch_explains_itself_both_ways() {
        for group in GROUPS {
            for key in group.keys {
                assert!(!key.label().is_empty());
                assert!(key.explanation().ends_with('.'), "{key:?}");
                assert!(key.consequence().ends_with('.'), "{key:?}");
                assert!(key.flag().starts_with("--"));
            }
        }
    }

    #[test]
    fn filesystem_places_are_offered_least_dangerous_first() {
        let names: Vec<&str> = FILESYSTEM_PRESETS.iter().map(|(id, _, _)| *id).collect();
        let home = names.iter().position(|id| *id == "home").unwrap();
        let host = names.iter().position(|id| *id == "host").unwrap();
        let documents = names.iter().position(|id| *id == "xdg-documents").unwrap();
        assert!(documents < home && home < host);
    }

    #[test]
    fn duplicate_places_are_not_added_twice() {
        let mut permissions = Permissions::default();
        permissions.add_filesystem("home");
        permissions.add_filesystem("home");
        permissions.add_filesystem("  ");
        assert_eq!(permissions.filesystems, vec!["home".to_string()]);

        permissions.remove_filesystem(0);
        assert!(permissions.filesystems.is_empty());
        permissions.remove_filesystem(5); // out of range, and not a panic
    }
}
