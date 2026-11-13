//! Search in the window: the find bar ([`bar`]) and its commands ([`find`]), Count, Replace
//! and Find All ([`replace`]), Find in Files and Replace in Files ([`files`]), the results
//! panel they fill ([`results`]), bookmarks, Mark and style tokens ([`marking`]), and smart
//! highlighting ([`smart`]).

pub mod bar;
mod files;
mod find;
pub(super) mod marking;
mod replace;
mod results;
mod smart;

use crate::window::Window;
use bar::BarMode;

pub use bar::FindBar;
pub use files::FilesSearch;
#[cfg(feature = "selftest")]
pub use replace::ReplaceState;
pub use results::ResultsPanel;

/// The commands of the Search menu, the find bar and the results panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchCommand {
    /// Opens the find bar.
    Find,
    FindNext,
    FindPrevious,
    /// Opens the find bar with the replace rows.
    Replace,
    /// Opens the find bar with the Find in Files rows.
    FindInFiles,
    ReplaceNext,
    ReplaceAll,
    ReplaceAllInOpenDocuments,
    Count,
    FindAllInDocument,
    FindAllInOpenDocuments,
    StopFindInFiles,
    ReplaceInFiles,
    /// Moves the focus between the editor and the results panel.
    SearchResults,
    NextResult,
    PreviousResult,
    CopyResults,
    ClearResults,
    CloseResults,
}

/// The find bar's and the results panel's own signals.
pub(super) fn connect(window: &Window) {
    find::connect(window);
    results::connect(window);
}

impl Window {
    /// The find bar's commands and the results panel's.
    pub fn run_search(&self, command: SearchCommand) {
        match command {
            SearchCommand::Find => self.show_find(),
            SearchCommand::FindNext => self.find_step(true),
            SearchCommand::FindPrevious => self.find_step(false),
            SearchCommand::Replace => self.show_find_mode(BarMode::Replace),
            SearchCommand::FindInFiles => self.show_find_mode(BarMode::Files),
            SearchCommand::ReplaceNext => self.replace_next(true),
            SearchCommand::ReplaceAll => self.replace_all(),
            SearchCommand::ReplaceAllInOpenDocuments => {
                let window = self.clone();
                self.spawn(async move { window.replace_all_in_open_documents().await });
            }
            SearchCommand::Count => self.count(),
            SearchCommand::FindAllInDocument => self.find_all(false),
            SearchCommand::FindAllInOpenDocuments => self.find_all(true),
            SearchCommand::StopFindInFiles => self.stop_files_searches(),
            SearchCommand::ReplaceInFiles => self.replace_in_files(),
            SearchCommand::SearchResults => self.toggle_results_focus(),
            SearchCommand::NextResult => self.step_result(true),
            SearchCommand::PreviousResult => self.step_result(false),
            SearchCommand::CopyResults => self.copy_results(),
            SearchCommand::ClearResults => self.clear_results(),
            SearchCommand::CloseResults => self.hide_results(),
        }
    }
}
