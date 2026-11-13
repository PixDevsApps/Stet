//! The banners above a page's text: one widget per kind, made on first use and stacked in
//! [`BannerKind`] order. What they say is decided elsewhere; the page only shows them.

use super::page::EditorPage;
use crate::banner::{Banner, BannerButton};
use gtk4::prelude::*;
use gtk4::subclass::prelude::*;
use std::rc::Rc;
use stet_domain::document::banners::BannerKind;

impl EditorPage {
    /// Shows the banner of `kind` with `title` and `buttons`, in its place in the stack.
    pub fn show_banner(&self, kind: BannerKind, title: &str, buttons: Vec<BannerButton>) {
        self.banner(kind).show(title, buttons);
    }

    pub fn hide_banner(&self, kind: BannerKind) {
        if let Some(banner) = self.imp().banners.borrow().get(&kind) {
            banner.hide();
        }
    }

    fn banner(&self, kind: BannerKind) -> Rc<Banner> {
        let imp = self.imp();
        if let Some(banner) = imp.banners.borrow().get(&kind) {
            return banner.clone();
        }
        let banner = Rc::new(Banner::new());
        let container = imp.banner_box.get().expect("constructed");
        let before = imp
            .banners
            .borrow()
            .range(..kind)
            .next_back()
            .map(|(_, banner)| banner.widget().clone());
        container.insert_child_after(banner.widget(), before.as_ref());
        imp.banners.borrow_mut().insert(kind, banner.clone());
        banner
    }

    /// The titles of the banners on screen, top to bottom.
    #[cfg(feature = "selftest")]
    pub fn banner_texts(&self) -> Vec<String> {
        self.imp()
            .banners
            .borrow()
            .values()
            .filter(|banner| banner.is_shown())
            .map(|banner| banner.title())
            .collect()
    }

    /// Clicks the button labelled `label` on a banner on screen.
    #[cfg(feature = "selftest")]
    pub fn click_banner(&self, label: &str) -> bool {
        let banners: Vec<Rc<Banner>> = self.imp().banners.borrow().values().cloned().collect();
        banners
            .iter()
            .filter(|banner| banner.is_shown())
            .any(|banner| banner.click(label))
    }

    /// The labels of the buttons on the banners on screen.
    #[cfg(feature = "selftest")]
    pub fn banner_buttons(&self) -> Vec<String> {
        self.imp()
            .banners
            .borrow()
            .values()
            .filter(|banner| banner.is_shown())
            .flat_map(|banner| banner.button_labels())
            .collect()
    }
}
