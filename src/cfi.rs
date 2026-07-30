//! Assembles EPUB CFIs from a located text range.

use crate::text::{Match, Step};

/// Builds the range CFI for a hit inside a spine item.
///
/// `spine_step` is the `<spine>` element's step in the package document and
/// `item_step` the `<itemref>`'s step within it — together the part before the
/// `!`. Highlights always span a range, so the `parent,start,end` form is used
/// even when both ends land in the same text node.
pub fn range(spine_step: usize, item_step: usize, hit: &Match) -> String {
    let shared = hit
        .start
        .path
        .iter()
        .zip(&hit.end.path)
        .take_while(|(a, b)| a.index == b.index)
        .count();

    // A range needs something on both sides of the split, so when one path is a
    // prefix of the other (both ends in the same text node) the last shared step
    // moves into the branches.
    let shared = shared.min(hit.start.path.len() - 1).min(hit.end.path.len() - 1);

    let parent = render(&hit.start.path[..shared]);
    let start = format!("{}:{}", render(&hit.start.path[shared..]), hit.start.utf16_offset);
    let end = format!("{}:{}", render(&hit.end.path[shared..]), hit.end.utf16_offset);

    format!("epubcfi(/{spine_step}/{item_step}!{parent},{start},{end})")
}

fn render(steps: &[Step]) -> String {
    steps
        .iter()
        .map(|step| match &step.id {
            Some(id) => format!("/{}[{}]", step.index, id),
            None => format!("/{}", step.index),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::range;
    use crate::text::{Match, Point, Step};

    fn step(index: usize, id: Option<&str>) -> Step {
        Step {
            index,
            id: id.map(str::to_string),
        }
    }

    #[test]
    fn matches_grimmory_output() {
        // The exact annotation grimmory produced for this book:
        // epubcfi(/6/14!/4/2[p01]/10,/1:909,/3:46)
        let hit = Match {
            start: Point {
                path: vec![
                    step(4, None),
                    step(2, Some("p01")),
                    step(10, None),
                    step(1, None),
                ],
                utf16_offset: 909,
            },
            end: Point {
                path: vec![
                    step(4, None),
                    step(2, Some("p01")),
                    step(10, None),
                    step(3, None),
                ],
                utf16_offset: 46,
            },
        };

        assert_eq!(
            range(6, 14, &hit),
            "epubcfi(/6/14!/4/2[p01]/10,/1:909,/3:46)"
        );
    }

    #[test]
    fn splits_when_both_ends_share_a_text_node() {
        let path = vec![step(4, None), step(2, None), step(1, None)];
        let hit = Match {
            start: Point {
                path: path.clone(),
                utf16_offset: 5,
            },
            end: Point {
                path,
                utf16_offset: 20,
            },
        };

        assert_eq!(range(6, 4, &hit), "epubcfi(/6/4!/4/2,/1:5,/1:20)");
    }
}
