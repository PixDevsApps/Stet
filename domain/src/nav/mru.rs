/// Documents in most-recently-used order, for Ctrl+Tab switching.
///
/// Pressing Ctrl+Tab calls [`next`](Self::next) (Ctrl+Shift+Tab [`prev`](Self::prev)), which
/// starts a switch at the most recent document and moves the selection through the order while
/// the modifier is held. Releasing the modifier calls [`commit`](Self::commit), which activates
/// the selection; Escape calls [`cancel`](Self::cancel). The order stays frozen during a switch,
/// so previewing documents does not reshuffle the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mru<T> {
    order: Vec<T>,
    selected: Option<usize>,
}

impl<T> Default for Mru<T> {
    fn default() -> Self {
        Self {
            order: Vec::new(),
            selected: None,
        }
    }
}

impl<T: PartialEq> Mru<T> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.order.len()
    }

    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    pub fn contains(&self, item: &T) -> bool {
        self.order.contains(item)
    }

    /// Most recent first.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.order.iter()
    }

    pub fn current(&self) -> Option<&T> {
        self.order.first()
    }

    /// Records that `item` became the active document. During a switch, known documents are
    /// ignored (the switcher previews them) and a new one goes to the front without moving the
    /// selection. Returns whether the order changed.
    pub fn activate(&mut self, item: T) -> bool {
        let position = self.order.iter().position(|known| *known == item);
        match (position, self.selected.as_mut()) {
            (Some(_), Some(_)) | (Some(0), None) => false,
            (Some(position), None) => {
                self.order[..=position].rotate_right(1);
                true
            }
            (None, selected) => {
                if let Some(selected) = selected {
                    *selected += 1;
                }
                self.order.insert(0, item);
                true
            }
        }
    }

    /// Adds a document that was opened without being shown (a tab restored in the background)
    /// as the least recently used one. Known documents keep their place. Returns whether the
    /// order changed.
    pub fn add_last(&mut self, item: T) -> bool {
        if self.order.contains(&item) {
            return false;
        }
        self.order.push(item);
        true
    }

    /// Forgets a closed document. A switch in progress selects the next document, or ends when
    /// none is left.
    pub fn remove(&mut self, item: &T) -> bool {
        let Some(position) = self.order.iter().position(|known| known == item) else {
            return false;
        };
        self.order.remove(position);
        if let Some(selected) = self.selected {
            self.selected = if self.order.is_empty() {
                None
            } else if position < selected || selected == self.order.len() {
                Some(selected - 1)
            } else {
                Some(selected)
            };
        }
        true
    }

    pub fn is_switching(&self) -> bool {
        self.selected.is_some()
    }

    /// Starts a switch with the current document selected. Does nothing during a switch.
    pub fn begin_switch(&mut self) -> Option<&T> {
        if self.selected.is_none() && !self.order.is_empty() {
            self.selected = Some(0);
        }
        self.selected()
    }

    /// Selects the next older document, wrapping to the most recent; starts a switch if needed.
    #[expect(
        clippy::should_implement_trait,
        reason = "a switcher step that wraps around, not an iterator"
    )]
    pub fn next(&mut self) -> Option<&T> {
        self.step(1)
    }

    /// Selects the next newer document, wrapping to the oldest; starts a switch if needed.
    pub fn prev(&mut self) -> Option<&T> {
        self.step(self.order.len().saturating_sub(1))
    }

    fn step(&mut self, by: usize) -> Option<&T> {
        self.begin_switch();
        if let Some(selected) = self.selected.as_mut() {
            *selected = (*selected + by) % self.order.len();
        }
        self.selected()
    }

    pub fn selected(&self) -> Option<&T> {
        self.selected.map(|index| &self.order[index])
    }

    /// The selection's position in [`iter`](Self::iter) order, for the switcher popup.
    pub fn selected_index(&self) -> Option<usize> {
        self.selected
    }

    /// Ends the switch and makes the selected document the most recent. Returns it.
    pub fn commit(&mut self) -> Option<&T> {
        let selected = self.selected.take()?;
        self.order[..=selected].rotate_right(1);
        self.order.first()
    }

    /// Ends the switch without changes. Returns the document that was current before it.
    pub fn cancel(&mut self) -> Option<&T> {
        self.selected = None;
        self.order.first()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn order(mru: &Mru<u32>) -> Vec<u32> {
        mru.iter().copied().collect()
    }

    fn opened(documents: &[u32]) -> Mru<u32> {
        let mut mru = Mru::new();
        for &document in documents {
            mru.activate(document);
        }
        mru
    }

    #[test]
    fn activation_moves_a_document_to_the_front() {
        let mut mru = opened(&[1, 2, 3]);
        assert_eq!(order(&mru), [3, 2, 1]);
        assert!(mru.activate(1));
        assert_eq!(order(&mru), [1, 3, 2]);
        assert!(!mru.activate(1));
        assert_eq!(mru.current(), Some(&1));
        assert!(mru.remove(&3));
        assert!(!mru.remove(&3));
        assert_eq!(order(&mru), [1, 2]);
    }

    #[test]
    fn ctrl_tab_once_switches_to_the_previous_document() {
        let mut mru = opened(&[1, 2, 3]);
        assert_eq!(mru.next(), Some(&2));
        assert!(mru.is_switching());
        assert_eq!(mru.commit(), Some(&2));
        assert!(!mru.is_switching());
        assert_eq!(order(&mru), [2, 3, 1]);
        assert_eq!(mru.next(), Some(&3));
        assert_eq!(mru.commit(), Some(&3));
        assert_eq!(order(&mru), [3, 2, 1]);
    }

    #[test]
    fn holding_the_modifier_cycles_and_wraps() {
        let mut mru = opened(&[1, 2, 3, 4]);
        assert_eq!(mru.begin_switch(), Some(&4));
        assert_eq!(mru.next(), Some(&3));
        assert_eq!(mru.next(), Some(&2));
        assert_eq!(mru.next(), Some(&1));
        assert_eq!(mru.next(), Some(&4));
        assert_eq!(mru.prev(), Some(&1));
        assert_eq!(mru.selected_index(), Some(3));
        assert_eq!(mru.commit(), Some(&1));
        assert_eq!(order(&mru), [1, 4, 3, 2]);
    }

    #[test]
    fn previews_do_not_reorder_and_cancel_restores() {
        let mut mru = opened(&[1, 2, 3]);
        mru.next();
        assert!(!mru.activate(2));
        mru.next();
        assert_eq!(mru.selected(), Some(&1));
        assert_eq!(mru.cancel(), Some(&3));
        assert_eq!(order(&mru), [3, 2, 1]);
        assert_eq!(mru.commit(), None);
    }

    #[test]
    fn documents_opened_or_closed_during_a_switch_keep_the_selection_sane() {
        let mut mru = opened(&[1, 2, 3]);
        mru.next();
        assert!(mru.activate(9));
        assert_eq!(mru.selected(), Some(&2));
        assert!(mru.remove(&2));
        assert_eq!(mru.selected(), Some(&1));
        assert!(mru.remove(&1));
        assert_eq!(mru.selected(), Some(&3));
        mru.remove(&3);
        mru.remove(&9);
        assert!(!mru.is_switching());
        assert_eq!(mru.next(), None);
    }

    #[test]
    fn background_documents_come_last_until_shown() {
        let mut mru = opened(&[1, 2]);
        assert!(mru.add_last(5));
        assert!(!mru.add_last(1));
        assert_eq!(order(&mru), [2, 1, 5]);
        mru.next();
        assert!(mru.add_last(6));
        assert_eq!(mru.selected(), Some(&1));
        assert_eq!(mru.prev(), Some(&2));
        assert_eq!(mru.prev(), Some(&6));
        assert_eq!(mru.commit(), Some(&6));
        assert_eq!(order(&mru), [6, 2, 1, 5]);
    }

    #[test]
    fn a_single_document_switches_to_itself() {
        let mut mru = opened(&[7]);
        assert_eq!(mru.next(), Some(&7));
        assert_eq!(mru.commit(), Some(&7));
        assert_eq!(order(&mru), [7]);
    }
}
