//! Add Game in fullscreen mode, step by step with a controller: what kind of game, then
//! the game's folder in fullscreen mode's own picker ([`crate::file_picker`]) and its
//! program, or a Windows installer to run first, then the title. This decides which step
//! shows and where B goes back to; the folder, programs and title are the shell's.

use std::path::{Path, PathBuf};

use crate::file_picker::Accept;

/// What kind of game is added, which decides the steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    /// A native game: its folder, then its program.
    Native,
    /// An installed Windows game, run through umu and Proton: its folder, then its
    /// program.
    Windows,
    /// A Windows game's installer, run through umu and Proton in a new prefix, then the
    /// program it installed.
    Installer,
}

impl Route {
    /// The files the picker takes: a folder for a game, a Windows program for an
    /// installer.
    pub fn accept(self) -> Accept {
        match self {
            Self::Native | Self::Windows => Accept::Folders,
            Self::Installer => Accept::WindowsPrograms,
        }
    }

    /// Whether the game runs through umu and Proton.
    pub fn windows(self) -> bool {
        self != Self::Native
    }
}

/// A step of Add Game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// What kind of game is added.
    Route,
    /// The places the picker starts from.
    Places,
    /// A folder's subfolders, and its programs on the installer route.
    Folder(PathBuf),
    /// The installer runs.
    Installing,
    /// The programs found in the game's folder, to pick the game's from.
    Programs,
    /// The title, and Add.
    Details,
}

/// Where Add Game is: the route and the steps taken, the last one showing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Flow {
    route: Route,
    steps: Vec<Step>,
}

impl Default for Flow {
    fn default() -> Self {
        Self::new()
    }
}

impl Flow {
    /// Add Game at its first step, the kind of game.
    pub fn new() -> Self {
        Self {
            route: Route::Native,
            steps: vec![Step::Route],
        }
    }

    /// The step that shows.
    pub fn step(&self) -> &Step {
        self.steps.last().unwrap_or(&Step::Route)
    }

    /// The kind of game being added.
    pub fn route(&self) -> Route {
        self.route
    }

    /// The folder the picker shows, while it shows one.
    pub fn folder(&self) -> Option<&Path> {
        match self.step() {
            Step::Folder(folder) => Some(folder),
            _ => None,
        }
    }

    /// The kind of game is chosen: on to the places.
    pub fn choose_route(&mut self, route: Route) {
        self.route = route;
        self.steps.push(Step::Places);
    }

    /// Opens `folder`, a place or a subfolder of the one shown.
    pub fn open(&mut self, folder: &Path) {
        self.steps.push(Step::Folder(folder.to_owned()));
    }

    /// The folder shown is the game's: on to its programs.
    pub fn use_folder(&mut self) {
        self.steps.push(Step::Programs);
    }

    /// An installer was picked and runs.
    pub fn start_installer(&mut self) {
        self.steps.push(Step::Installing);
    }

    /// The installer ended: on to the programs it installed when it `added` any, which
    /// lead back to the start, as the installer cannot be undone; else to picking the
    /// installed game's folder by hand.
    pub fn installed(&mut self, added: bool) {
        self.steps.truncate(1);
        if added {
            self.steps.push(Step::Programs);
        } else {
            self.choose_route(Route::Windows);
        }
    }

    /// The installer could not run: back to the folder it was picked in.
    pub fn installer_failed(&mut self) {
        if self.step() == &Step::Installing {
            self.steps.pop();
        }
    }

    /// The game's program is picked: on to its title.
    pub fn choose_program(&mut self) {
        self.steps.push(Step::Details);
    }

    /// B: back a step, up a folder, or nothing while the installer runs. `false` on the
    /// first step, which B closes.
    pub fn back(&mut self) -> bool {
        match self.step() {
            Step::Route => false,
            Step::Installing => true,
            _ => {
                self.steps.pop();
                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn folder(path: &str) -> Step {
        Step::Folder(path.into())
    }

    #[test]
    fn a_game_folder_is_picked_from_the_places_down_and_b_goes_back_up() {
        let mut flow = Flow::new();
        assert_eq!(flow.step(), &Step::Route);
        flow.choose_route(Route::Native);
        assert_eq!(flow.step(), &Step::Places);
        flow.open(Path::new("/home/me/Games"));
        flow.open(Path::new("/home/me/Games/Celeste"));
        assert_eq!(flow.step(), &folder("/home/me/Games/Celeste"));
        assert_eq!(flow.folder(), Some(Path::new("/home/me/Games/Celeste")));
        flow.use_folder();
        assert_eq!(flow.step(), &Step::Programs);
        flow.choose_program();
        assert_eq!(flow.step(), &Step::Details);
        assert!(flow.back());
        assert_eq!(flow.step(), &Step::Programs);
        assert!(flow.back());
        assert_eq!(flow.step(), &folder("/home/me/Games/Celeste"));
        assert!(flow.back());
        assert_eq!(flow.step(), &folder("/home/me/Games"));
        assert!(flow.back());
        assert_eq!(flow.step(), &Step::Places);
        assert_eq!(flow.folder(), None);
        assert!(flow.back());
        assert_eq!(flow.step(), &Step::Route);
        assert!(!flow.back(), "B on the first step closes Add Game");
        assert_eq!(flow.step(), &Step::Route);
    }

    #[test]
    fn the_route_says_which_files_the_picker_takes_and_whether_the_game_runs_on_proton() {
        assert_eq!(Route::Native.accept(), Accept::Folders);
        assert_eq!(Route::Windows.accept(), Accept::Folders);
        assert_eq!(Route::Installer.accept(), Accept::WindowsPrograms);
        assert!(!Route::Native.windows());
        assert!(Route::Windows.windows());
        assert!(Route::Installer.windows());
        let mut flow = Flow::new();
        flow.choose_route(Route::Installer);
        assert_eq!(flow.route(), Route::Installer);
    }

    #[test]
    fn an_installer_runs_without_a_way_back_then_its_programs_lead_back_to_the_start() {
        let mut flow = Flow::new();
        flow.choose_route(Route::Installer);
        flow.open(Path::new("/run/media/me/USB"));
        flow.start_installer();
        assert_eq!(flow.step(), &Step::Installing);
        assert!(flow.back(), "B waits while the installer runs");
        assert_eq!(flow.step(), &Step::Installing);
        flow.installed(true);
        assert_eq!(flow.step(), &Step::Programs);
        assert_eq!(flow.route(), Route::Installer);
        flow.choose_program();
        assert!(flow.back());
        assert!(flow.back());
        assert_eq!(flow.step(), &Step::Route);
    }

    #[test]
    fn an_installer_that_added_nothing_leads_to_picking_the_game_s_folder() {
        let mut flow = Flow::new();
        flow.choose_route(Route::Installer);
        flow.open(Path::new("/home/me/Games"));
        flow.start_installer();
        flow.installed(false);
        assert_eq!(flow.step(), &Step::Places);
        assert_eq!(flow.route(), Route::Windows);
        assert!(flow.back());
        assert_eq!(flow.step(), &Step::Route);
    }

    #[test]
    fn an_installer_that_could_not_run_goes_back_to_its_folder() {
        let mut flow = Flow::new();
        flow.choose_route(Route::Installer);
        flow.open(Path::new("/home/me/Games"));
        flow.start_installer();
        flow.installer_failed();
        assert_eq!(flow.step(), &folder("/home/me/Games"));
        assert_eq!(flow.route(), Route::Installer);
    }
}
