//! Shared driver for elements that expand into generated parts (curtain wall
//! grids, stairs with landings): each part is inserted right after its whole.

use std::collections::HashSet;

use super::diagnostics::Diagnostic;
use super::model::{ResolvedElement, SemanticModel};

/// Generates the parts of one whole from its storey height and the ids in
/// use, or the error that leaves it unexpanded.
pub(super) type Expand = fn(
    &mut ResolvedElement,
    f64,
    &HashSet<String>,
) -> Result<Vec<ResolvedElement>, Box<Diagnostic>>;

/// Run `expand` on every element matching `is_whole`. Returns the errors.
pub(super) fn expand_all(
    model: &mut SemanticModel,
    is_whole: fn(&ResolvedElement) -> bool,
    expand: Expand,
) -> Vec<Diagnostic> {
    if !model.elements().any(is_whole) {
        return vec![];
    }
    let ids: HashSet<String> = model
        .elements()
        .flat_map(|e| std::iter::once(e).chain(&e.children))
        .map(|e| e.id.clone())
        .collect();
    let heights: Vec<f64> = model.containers().map(|c| c.height()).collect();
    let mut out = Vec::new();
    for ((_, spaces, elements), height) in model.containers_mut().zip(heights) {
        let lists = std::iter::once(elements).chain(spaces.iter_mut().map(|s| &mut s.elements));
        for list in lists.filter(|l| l.iter().any(is_whole)) {
            for mut elem in std::mem::take(list) {
                let parts = if is_whole(&elem) {
                    expand(&mut elem, height, &ids).unwrap_or_else(|d| {
                        out.push(*d);
                        vec![]
                    })
                } else {
                    vec![]
                };
                list.push(elem);
                list.extend(parts);
            }
        }
    }
    out
}
