use crate::domain::Track;

/// Select new automatic-mix tracks without duplicating the current, queued,
/// or previously played IDs.
pub(crate) fn select_mix_items(
    items: impl IntoIterator<Item = Track>,
    current_id: Option<&str>,
    queued_ids: &[&str],
    history_ids: &[&str],
    max_items: usize,
) -> Vec<Track> {
    let mut selected = Vec::new();
    for item in items {
        if current_id == Some(item.id.as_str())
            || queued_ids.contains(&item.id.as_str())
            || history_ids.contains(&item.id.as_str())
            || selected.iter().any(|track: &Track| track.id == item.id)
        {
            continue;
        }
        selected.push(item);
        if selected.len() >= max_items {
            break;
        }
    }
    selected
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(id: &str) -> Track {
        Track {
            title: id.into(),
            id: id.into(),
            duration: Some(120.0),
            channel: "Test".into(),
        }
    }

    #[test]
    fn mix_selection_excludes_current_queue_history_and_duplicates() {
        let items = vec![
            track("current"),
            track("queued"),
            track("played"),
            track("new"),
            track("new"),
        ];
        let selected = select_mix_items(items, Some("current"), &["queued"], &["played"], 8);
        assert_eq!(
            selected
                .iter()
                .map(|track| track.id.as_str())
                .collect::<Vec<_>>(),
            ["new"]
        );
    }

    #[test]
    fn mix_selection_respects_maximum() {
        let selected = select_mix_items(
            (0..4).map(|index| track(&index.to_string())),
            None,
            &[],
            &[],
            2,
        );
        assert_eq!(selected.len(), 2);
    }
}
