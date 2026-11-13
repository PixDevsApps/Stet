//! Routes every registry action to the window feature that runs it. The match below has no
//! wildcard: a new [`ActionId`] does not compile until it is routed here, and each feature
//! matches its own command enum exhaustively.

use super::Window;
use super::chrome::language::LanguageCommand;
use super::documents::encoding::{EncodingCommand, FixedEncoding};
use super::documents::files::FileCommand;
use super::search::SearchCommand;
use super::search::marking::MarkCommand;
use super::text::editing::{ColumnCommand, EditCommand};
use super::text::toolbox::TextTool;
use super::tools::compare::CompareCommand;
use super::workspace::nav::NavCommand;
use super::workspace::pins::PinCommand;
use super::workspace::split::SplitCommand;
use super::workspace::tabs::{TabCommand, TabTool};
use super::workspace::view::ViewCommand;
use crate::editor::column::Motion;
use std::path::PathBuf;
use stet_domain::actions::ActionId;
use stet_domain::marks::MarkStyle;
use stet_domain::text::Eol;

impl Window {
    /// Runs a window action. `param` is the string parameter of `WithString` actions.
    pub fn run_action(&self, id: ActionId, param: Option<String>) {
        // Column mode closes its typing burst first, and ends for actions that don't know
        // about columns.
        if let Some(page) = self.current_page()
            && crate::editor::column::before_action(page.column_view(), id)
        {
            return;
        }
        match id {
            ActionId::NewTab => self.run_file(FileCommand::NewTab),
            ActionId::Open => self.run_file(FileCommand::Open),
            ActionId::QuickOpen => self.open_quick_open(""),
            ActionId::NextRecentTab => self.run_nav(NavCommand::NextRecentTab),
            ActionId::PreviousRecentTab => self.run_nav(NavCommand::PreviousRecentTab),
            ActionId::GoBack => self.run_nav(NavCommand::Back),
            ActionId::GoForward => self.run_nav(NavCommand::Forward),
            ActionId::PinTab => self.run_pin(PinCommand::Pin),
            ActionId::UnpinTab => self.run_pin(PinCommand::Unpin),
            ActionId::OpenRecent => {
                if let Some(path) = param {
                    self.run_file(FileCommand::OpenRecent(PathBuf::from(path)));
                }
            }
            ActionId::ClearRecent => self.run_file(FileCommand::ClearRecent),
            ActionId::Save => self.run_file(FileCommand::Save),
            ActionId::SaveAs => self.run_file(FileCommand::SaveAs),
            ActionId::SaveAll => self.run_file(FileCommand::SaveAll),
            ActionId::CloseTab => self.run_tab_tool(TabTool::Close),
            ActionId::CloseAll => self.run_tab(TabCommand::CloseAll),
            ActionId::RestoreClosedTab => self.run_tab(TabCommand::RestoreClosed),
            ActionId::ForgetDrafts => self.forget_drafts(),
            ActionId::Undo => self.run_edit(EditCommand::Undo),
            ActionId::Redo => self.run_edit(EditCommand::Redo),
            ActionId::Cut => self.run_edit(EditCommand::Cut),
            ActionId::Copy => self.run_edit(EditCommand::Copy),
            ActionId::Paste => self.run_edit(EditCommand::Paste),
            ActionId::Delete => self.run_edit(EditCommand::Delete),
            ActionId::SelectAll => self.run_edit(EditCommand::SelectAll),
            ActionId::Find => self.run_search(SearchCommand::Find),
            ActionId::FindNext => self.run_search(SearchCommand::FindNext),
            ActionId::FindPrevious => self.run_search(SearchCommand::FindPrevious),
            ActionId::FindInFiles => self.run_search(SearchCommand::FindInFiles),
            ActionId::Replace => self.run_search(SearchCommand::Replace),
            ActionId::ReplaceNext => self.run_search(SearchCommand::ReplaceNext),
            ActionId::ReplaceAll => self.run_search(SearchCommand::ReplaceAll),
            ActionId::ReplaceAllInOpenDocuments => {
                self.run_search(SearchCommand::ReplaceAllInOpenDocuments);
            }
            ActionId::Count => self.run_search(SearchCommand::Count),
            ActionId::FindAllInDocument => self.run_search(SearchCommand::FindAllInDocument),
            ActionId::FindAllInOpenDocuments => {
                self.run_search(SearchCommand::FindAllInOpenDocuments);
            }
            ActionId::StopFindInFiles => self.run_search(SearchCommand::StopFindInFiles),
            ActionId::ReplaceInFiles => self.run_search(SearchCommand::ReplaceInFiles),
            ActionId::SearchResults => self.run_search(SearchCommand::SearchResults),
            ActionId::NextSearchResult => self.run_search(SearchCommand::NextResult),
            ActionId::PreviousSearchResult => self.run_search(SearchCommand::PreviousResult),
            ActionId::CopySearchResults => self.run_search(SearchCommand::CopyResults),
            ActionId::ClearSearchResults => self.run_search(SearchCommand::ClearResults),
            ActionId::CloseSearchResults => self.run_search(SearchCommand::CloseResults),
            ActionId::GoToLine => self.open_palette(":"),
            ActionId::WordWrap => self.run_view(ViewCommand::WordWrap),
            ActionId::ShowWhitespace => self.run_view(ViewCommand::ShowWhitespace),
            ActionId::ZoomIn => self.run_view(ViewCommand::ZoomIn),
            ActionId::ZoomOut => self.run_view(ViewCommand::ZoomOut),
            ActionId::ZoomReset => self.run_view(ViewCommand::ZoomReset),
            ActionId::FullScreen => self.run_view(ViewCommand::FullScreen),
            ActionId::NextTab => self.run_tab(TabCommand::Next),
            ActionId::PreviousTab => self.run_tab(TabCommand::Previous),
            ActionId::MoveTabForward => self.run_tab(TabCommand::MoveForward),
            ActionId::MoveTabBackward => self.run_tab(TabCommand::MoveBackward),
            ActionId::CommandPalette => self.open_palette(""),
            ActionId::ChooseLanguage => self.run_language(LanguageCommand::Choose),
            ActionId::SetLanguage => self.run_language(LanguageCommand::Set(param)),
            ActionId::ReloadFromDisk => self.run_file(FileCommand::ReloadFromDisk),
            ActionId::EolCrLf => self.run_encoding(EncodingCommand::SetEol(Eol::CrLf)),
            ActionId::EolLf => self.run_encoding(EncodingCommand::SetEol(Eol::Lf)),
            ActionId::EolCr => self.run_encoding(EncodingCommand::SetEol(Eol::Cr)),
            ActionId::EncodeUtf8 => self.run_encoding(EncodingCommand::Encode(FixedEncoding::Utf8)),
            ActionId::EncodeUtf8Bom => {
                self.run_encoding(EncodingCommand::Encode(FixedEncoding::Utf8Bom));
            }
            ActionId::EncodeUtf16BeBom => {
                self.run_encoding(EncodingCommand::Encode(FixedEncoding::Utf16BeBom));
            }
            ActionId::EncodeUtf16LeBom => {
                self.run_encoding(EncodingCommand::Encode(FixedEncoding::Utf16LeBom));
            }
            ActionId::Reinterpret => self.run_encoding(EncodingCommand::Reinterpret(param)),
            ActionId::ConvertToAnsi => {
                self.run_encoding(EncodingCommand::ConvertTo(FixedEncoding::Ansi));
            }
            ActionId::ConvertToUtf8 => {
                self.run_encoding(EncodingCommand::ConvertTo(FixedEncoding::Utf8));
            }
            ActionId::ConvertToUtf8Bom => {
                self.run_encoding(EncodingCommand::ConvertTo(FixedEncoding::Utf8Bom));
            }
            ActionId::ConvertToUtf16BeBom => {
                self.run_encoding(EncodingCommand::ConvertTo(FixedEncoding::Utf16BeBom));
            }
            ActionId::ConvertToUtf16LeBom => {
                self.run_encoding(EncodingCommand::ConvertTo(FixedEncoding::Utf16LeBom));
            }
            ActionId::ConvertEncoding => self.run_encoding(EncodingCommand::Convert(param)),
            ActionId::ChooseEncoding => self.run_encoding(EncodingCommand::Choose),
            ActionId::About => self.show_about(),
            ActionId::CheckForUpdates => self.check_for_updates(),
            ActionId::CloseOthers => self.run_tab_tool(TabTool::CloseOthers),
            ActionId::CloseToTheRight => self.run_tab_tool(TabTool::CloseToTheRight),
            ActionId::CopyFullPath => self.run_tab_tool(TabTool::CopyFullPath),
            ActionId::CopyFileName => self.run_tab_tool(TabTool::CopyFileName),
            ActionId::CopyDirectoryPath => self.run_tab_tool(TabTool::CopyDirectoryPath),
            ActionId::OpenContainingFolder => self.run_tab_tool(TabTool::OpenContainingFolder),
            ActionId::OpenTerminalHere => self.run_tab_tool(TabTool::OpenTerminalHere),
            ActionId::RenameFile => self.run_tab_tool(TabTool::Rename),
            ActionId::MoveToTrash => self.run_tab_tool(TabTool::MoveToTrash),
            ActionId::Uppercase => self.run_text_tool(TextTool::Uppercase),
            ActionId::Lowercase => self.run_text_tool(TextTool::Lowercase),
            ActionId::ProperCase => self.run_text_tool(TextTool::ProperCase),
            ActionId::ProperCaseBlend => self.run_text_tool(TextTool::ProperCaseBlend),
            ActionId::SentenceCase => self.run_text_tool(TextTool::SentenceCase),
            ActionId::SentenceCaseBlend => self.run_text_tool(TextTool::SentenceCaseBlend),
            ActionId::InvertCase => self.run_text_tool(TextTool::InvertCase),
            ActionId::DuplicateLine => self.run_text_tool(TextTool::DuplicateLine),
            ActionId::CutLine => self.run_text_tool(TextTool::CutLine),
            ActionId::CopyLine => self.run_text_tool(TextTool::CopyLine),
            ActionId::DeleteLine => self.run_text_tool(TextTool::DeleteLine),
            ActionId::TransposeLine => self.run_text_tool(TextTool::TransposeLine),
            ActionId::MoveLineUp => self.run_text_tool(TextTool::MoveLineUp),
            ActionId::MoveLineDown => self.run_text_tool(TextTool::MoveLineDown),
            ActionId::JoinLines => self.run_text_tool(TextTool::JoinLines),
            ActionId::BlankLineAbove => self.run_text_tool(TextTool::BlankLineAbove),
            ActionId::BlankLineBelow => self.run_text_tool(TextTool::BlankLineBelow),
            ActionId::RemoveDuplicateLines => self.run_text_tool(TextTool::RemoveDuplicateLines),
            ActionId::RemoveConsecutiveDuplicateLines => {
                self.run_text_tool(TextTool::RemoveConsecutiveDuplicateLines);
            }
            ActionId::RemoveEmptyLines => self.run_text_tool(TextTool::RemoveEmptyLines),
            ActionId::RemoveBlankLines => self.run_text_tool(TextTool::RemoveBlankLines),
            ActionId::ReverseLines => self.run_text_tool(TextTool::ReverseLines),
            ActionId::SortLexicalAscending => self.run_text_tool(TextTool::SortLexicalAscending),
            ActionId::SortLexicalDescending => {
                self.run_text_tool(TextTool::SortLexicalDescending);
            }
            ActionId::SortIgnoreCaseAscending => {
                self.run_text_tool(TextTool::SortIgnoreCaseAscending);
            }
            ActionId::SortIgnoreCaseDescending => {
                self.run_text_tool(TextTool::SortIgnoreCaseDescending);
            }
            ActionId::SortIntegerAscending => self.run_text_tool(TextTool::SortIntegerAscending),
            ActionId::SortIntegerDescending => {
                self.run_text_tool(TextTool::SortIntegerDescending);
            }
            ActionId::SortDecimalCommaAscending => {
                self.run_text_tool(TextTool::SortDecimalCommaAscending);
            }
            ActionId::SortDecimalCommaDescending => {
                self.run_text_tool(TextTool::SortDecimalCommaDescending);
            }
            ActionId::SortDecimalDotAscending => {
                self.run_text_tool(TextTool::SortDecimalDotAscending);
            }
            ActionId::SortDecimalDotDescending => {
                self.run_text_tool(TextTool::SortDecimalDotDescending);
            }
            ActionId::SortLengthAscending => self.run_text_tool(TextTool::SortLengthAscending),
            ActionId::SortLengthDescending => self.run_text_tool(TextTool::SortLengthDescending),
            ActionId::ToggleComment => self.run_text_tool(TextTool::ToggleComment),
            ActionId::CommentLines => self.run_text_tool(TextTool::CommentLines),
            ActionId::UncommentLines => self.run_text_tool(TextTool::UncommentLines),
            ActionId::ToggleBlockComment => self.run_text_tool(TextTool::ToggleBlockComment),
            ActionId::TrimTrailing => self.run_text_tool(TextTool::TrimTrailing),
            ActionId::TrimLeading => self.run_text_tool(TextTool::TrimLeading),
            ActionId::TrimBoth => self.run_text_tool(TextTool::TrimBoth),
            ActionId::TabsToSpaces => self.run_text_tool(TextTool::TabsToSpaces),
            ActionId::SpacesToTabs => self.run_text_tool(TextTool::SpacesToTabs),
            ActionId::SpacesToTabsLeading => self.run_text_tool(TextTool::SpacesToTabsLeading),
            ActionId::AddPrefixSuffix => self.run_text_tool(TextTool::AddPrefixSuffix),
            ActionId::InsertNumbers => self.run_text_tool(TextTool::InsertNumbers),
            ActionId::GoToMatchingBrace => self.run_text_tool(TextTool::GoToMatchingBrace),
            ActionId::SelectToMatchingBrace => {
                self.run_text_tool(TextTool::SelectToMatchingBrace);
            }
            ActionId::SelectAndFindNext => self.run_text_tool(TextTool::SelectAndFindNext),
            ActionId::SelectAndFindPrevious => {
                self.run_text_tool(TextTool::SelectAndFindPrevious);
            }
            ActionId::FormatJson => self.run_text_tool(TextTool::FormatJson),
            ActionId::MinifyJson => self.run_text_tool(TextTool::MinifyJson),
            ActionId::ValidateJson => self.run_text_tool(TextTool::ValidateJson),
            ActionId::FormatXml => self.run_text_tool(TextTool::FormatXml),
            ActionId::ValidateXml => self.run_text_tool(TextTool::ValidateXml),
            ActionId::DocumentMap => self.run_view(ViewCommand::DocumentMap),
            ActionId::ChooseIndentation => self.choose_indentation(),
            ActionId::DetectIndentation => self.detect_indentation(),
            ActionId::OpenSettings => self.open_settings_file(false),
            ActionId::OpenKeyboardShortcuts => self.open_settings_file(true),
            ActionId::SetAsDefaultEditor => {
                let window = self.clone();
                self.spawn(async move { window.set_as_default_editor().await });
            }
            ActionId::KeyboardShortcuts => self.show_keyboard_shortcuts(),
            ActionId::ColumnSelectLeft => self.run_column(ColumnCommand::Select(Motion::Left)),
            ActionId::ColumnSelectRight => self.run_column(ColumnCommand::Select(Motion::Right)),
            ActionId::ColumnSelectUp => self.run_column(ColumnCommand::Select(Motion::Up)),
            ActionId::ColumnSelectDown => self.run_column(ColumnCommand::Select(Motion::Down)),
            ActionId::ColumnSelectLineStart => {
                self.run_column(ColumnCommand::Select(Motion::LineStart));
            }
            ActionId::ColumnSelectLineEnd => {
                self.run_column(ColumnCommand::Select(Motion::LineEnd));
            }
            ActionId::ColumnSelectPageUp => {
                self.run_column(ColumnCommand::Select(Motion::PageUp));
            }
            ActionId::ColumnSelectPageDown => {
                self.run_column(ColumnCommand::Select(Motion::PageDown));
            }
            ActionId::ColumnBeginEndSelect => self.run_column(ColumnCommand::BeginEndSelect),
            ActionId::ColumnEditor => self.run_column(ColumnCommand::Editor),
            ActionId::ToggleBookmark => self.run_mark(MarkCommand::ToggleBookmark),
            ActionId::NextBookmark => self.run_mark(MarkCommand::NextBookmark),
            ActionId::PreviousBookmark => self.run_mark(MarkCommand::PreviousBookmark),
            ActionId::ClearBookmarks => self.run_mark(MarkCommand::ClearBookmarks),
            ActionId::CutBookmarkedLines => self.run_mark(MarkCommand::CutBookmarkedLines),
            ActionId::CopyBookmarkedLines => self.run_mark(MarkCommand::CopyBookmarkedLines),
            ActionId::PasteToBookmarkedLines => {
                self.run_mark(MarkCommand::PasteToBookmarkedLines);
            }
            ActionId::RemoveBookmarkedLines => self.run_mark(MarkCommand::RemoveBookmarkedLines),
            ActionId::RemoveUnbookmarkedLines => {
                self.run_mark(MarkCommand::RemoveUnbookmarkedLines);
            }
            ActionId::InverseBookmarks => self.run_mark(MarkCommand::InverseBookmarks),
            ActionId::Mark => self.run_mark(MarkCommand::Mark),
            ActionId::MarkAll => self.run_mark(MarkCommand::MarkAll),
            ActionId::ClearMarks => self.run_mark(MarkCommand::ClearMarks),
            ActionId::CopyMarkedText => self.run_mark(MarkCommand::CopyMarkedText),
            ActionId::StyleToken1 => self.run_mark(MarkCommand::StyleToken(MarkStyle::Token1)),
            ActionId::StyleToken2 => self.run_mark(MarkCommand::StyleToken(MarkStyle::Token2)),
            ActionId::StyleToken3 => self.run_mark(MarkCommand::StyleToken(MarkStyle::Token3)),
            ActionId::StyleToken4 => self.run_mark(MarkCommand::StyleToken(MarkStyle::Token4)),
            ActionId::StyleToken5 => self.run_mark(MarkCommand::StyleToken(MarkStyle::Token5)),
            ActionId::ClearStyle1 => self.run_mark(MarkCommand::ClearStyle(MarkStyle::Token1)),
            ActionId::ClearStyle2 => self.run_mark(MarkCommand::ClearStyle(MarkStyle::Token2)),
            ActionId::ClearStyle3 => self.run_mark(MarkCommand::ClearStyle(MarkStyle::Token3)),
            ActionId::ClearStyle4 => self.run_mark(MarkCommand::ClearStyle(MarkStyle::Token4)),
            ActionId::ClearStyle5 => self.run_mark(MarkCommand::ClearStyle(MarkStyle::Token5)),
            ActionId::ClearAllStyles => self.run_mark(MarkCommand::ClearAllStyles),
            ActionId::JumpUp1 => self.run_mark(MarkCommand::JumpToStyle {
                style: MarkStyle::Token1,
                down: false,
            }),
            ActionId::JumpUp2 => self.run_mark(MarkCommand::JumpToStyle {
                style: MarkStyle::Token2,
                down: false,
            }),
            ActionId::JumpUp3 => self.run_mark(MarkCommand::JumpToStyle {
                style: MarkStyle::Token3,
                down: false,
            }),
            ActionId::JumpUp4 => self.run_mark(MarkCommand::JumpToStyle {
                style: MarkStyle::Token4,
                down: false,
            }),
            ActionId::JumpUp5 => self.run_mark(MarkCommand::JumpToStyle {
                style: MarkStyle::Token5,
                down: false,
            }),
            ActionId::JumpUpMark => self.run_mark(MarkCommand::JumpToStyle {
                style: MarkStyle::Mark,
                down: false,
            }),
            ActionId::JumpDown1 => self.run_mark(MarkCommand::JumpToStyle {
                style: MarkStyle::Token1,
                down: true,
            }),
            ActionId::JumpDown2 => self.run_mark(MarkCommand::JumpToStyle {
                style: MarkStyle::Token2,
                down: true,
            }),
            ActionId::JumpDown3 => self.run_mark(MarkCommand::JumpToStyle {
                style: MarkStyle::Token3,
                down: true,
            }),
            ActionId::JumpDown4 => self.run_mark(MarkCommand::JumpToStyle {
                style: MarkStyle::Token4,
                down: true,
            }),
            ActionId::JumpDown5 => self.run_mark(MarkCommand::JumpToStyle {
                style: MarkStyle::Token5,
                down: true,
            }),
            ActionId::JumpDownMark => self.run_mark(MarkCommand::JumpToStyle {
                style: MarkStyle::Mark,
                down: true,
            }),
            ActionId::MoveToOtherView => self.run_split(SplitCommand::MoveToOtherView),
            ActionId::CloneToOtherView => self.run_split(SplitCommand::CloneToOtherView),
            ActionId::SwitchView => self.run_split(SplitCommand::SwitchView),
            ActionId::SyncVerticalScrolling => self.run_split(SplitCommand::SyncVerticalScrolling),
            ActionId::SyncHorizontalScrolling => {
                self.run_split(SplitCommand::SyncHorizontalScrolling);
            }
            ActionId::Compare => self.run_compare(CompareCommand::WithOtherView),
            ActionId::CompareWithFile => self.run_compare(CompareCommand::WithFile),
            ActionId::CompareWithClipboard => self.run_compare(CompareCommand::WithClipboard),
            ActionId::CompareWithSaved => self.run_compare(CompareCommand::WithSaved),
            ActionId::PreviousDifference => self.run_compare(CompareCommand::PreviousDifference),
            ActionId::NextDifference => self.run_compare(CompareCommand::NextDifference),
            ActionId::CompareIgnoreWhitespace => {
                self.run_compare(CompareCommand::IgnoreWhitespace);
            }
            ActionId::CompareIgnoreCase => self.run_compare(CompareCommand::IgnoreCase),
            ActionId::ClearCompare => self.run_compare(CompareCommand::Clear),
            // Application-scoped: `app.quit` closes every window (`crate::actions`); a window
            // has no action of its own for it.
            ActionId::Quit => {}
        }
    }
}
