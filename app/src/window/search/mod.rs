//! Search in the window: the find bar ([`bar`]) and its commands ([`find`]), Count, Replace
//! and Find All ([`replace`]), Find in Files and Replace in Files ([`files`]), and the results
//! panel they fill ([`results`]).

pub mod bar;
mod files;
mod find;
mod replace;
mod results;

use crate::window::Window;
use bar::BarMode;
use stet_domain::actions::ActionId;

pub use bar::FindBar;
pub use files::FilesSearch;
#[cfg(feature = "selftest")]
pub use replace::ReplaceState;
pub use results::ResultsPanel;

impl Window {
    /// The find bar's commands and the results panel's.
    pub fn run_search_action(&self, id: ActionId) {
        match id {
            ActionId::Replace => self.show_find_mode(BarMode::Replace),
            ActionId::FindInFiles => self.show_find_mode(BarMode::Files),
            ActionId::ReplaceNext => self.replace_next(true),
            ActionId::ReplaceAll => self.replace_all(),
            ActionId::ReplaceAllInOpenDocuments => {
                let window = self.clone();
                self.spawn(async move { window.replace_all_in_open_documents().await });
            }
            ActionId::Count => self.count(),
            ActionId::FindAllInDocument => self.find_all(false),
            ActionId::FindAllInOpenDocuments => self.find_all(true),
            ActionId::StopFindInFiles => self.stop_files_searches(),
            ActionId::ReplaceInFiles => self.replace_in_files(),
            ActionId::SearchResults => self.toggle_results_focus(),
            ActionId::NextSearchResult => self.step_result(true),
            ActionId::PreviousSearchResult => self.step_result(false),
            ActionId::CopySearchResults => self.copy_results(),
            ActionId::ClearSearchResults => self.clear_results(),
            ActionId::CloseSearchResults => self.hide_results(),
            _ => {}
        }
    }
}
