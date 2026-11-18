//! Whole workbooks

use super::*;

/// A formula waiting for its result: its text, and how far it is copied from where it was
/// written (shared formulas)
#[derive(Clone, Debug)]
pub struct Pending {
    pub text: std::sync::Arc<str>,
    pub shift: (i64, i64),
}

/// The values of one workbook's sheets, with formulas worked out on demand (each once,
/// in whatever order they refer to each other)
pub struct Book<'a> {
    pub names: Vec<String>,
    pub values: Vec<&'a std::collections::BTreeMap<(u32, u32), crate::office::xlsx::Cell>>,
    pub pending: Vec<&'a HashMap<(u32, u32), Pending>>,
    pub date1904: bool,
    pub sep: Separators,
    pub(super) results: std::cell::RefCell<HashMap<(usize, u32, u32), V>>,
    pub(super) parsed: std::cell::RefCell<HashMap<*const str, std::rc::Rc<Expr>>>,
    pub(super) busy: std::cell::RefCell<std::collections::HashSet<(usize, u32, u32)>>,
    pub(super) extents: Vec<(u32, u32)>,
}

impl<'a> Book<'a> {
    pub fn new(names: Vec<String>, values: Vec<&'a std::collections::BTreeMap<(u32, u32), crate::office::xlsx::Cell>>, pending: Vec<&'a HashMap<(u32, u32), Pending>>, date1904: bool) -> Self {
        let extents = values
            .iter()
            .zip(&pending)
            .map(|(cells, p)| {
                let keys = cells.keys().chain(p.keys());
                keys.fold((0, 0), |(r, c), &(kr, kc)| (r.max(kr), c.max(kc)))
            })
            .collect();
        Book {
            names,
            values,
            pending,
            date1904,
            sep: Separators::system(),
            results: Default::default(),
            parsed: Default::default(),
            busy: Default::default(),
            extents,
        }
    }

    /// The result of the formula in a cell
    pub fn result(&self, sheet: usize, r: u32, c: u32) -> V {
        if let Some(v) = self.results.borrow().get(&(sheet, r, c)) {
            return v.clone();
        }
        let Some(p) = self.pending[sheet].get(&(r, c)) else { return V::Blank };
        {
            // a formula that (indirectly) refers to itself, or a chain too deep to follow
            // here (cells are worked out top to bottom, so chains stay short)
            let mut busy = self.busy.borrow_mut();
            if busy.len() > 200 || !busy.insert((sheet, r, c)) {
                return V::Err(UNSUPPORTED);
            }
        }
        let expr = self.expr(p);
        let v = Eval { cells: self, sheet, here: (r, c), date1904: self.date1904, sep: self.sep }.value(&expr);
        self.busy.borrow_mut().remove(&(sheet, r, c));
        self.results.borrow_mut().insert((sheet, r, c), v.clone());
        v
    }

    fn expr(&self, p: &Pending) -> std::rc::Rc<Expr> {
        let key = std::sync::Arc::as_ptr(&p.text);
        let base = self.parsed.borrow().get(&key).cloned();
        let base = base.unwrap_or_else(|| {
            let e = std::rc::Rc::new(parse(&p.text));
            self.parsed.borrow_mut().insert(key, e.clone());
            e
        });
        if p.shift == (0, 0) { base } else { std::rc::Rc::new(base.shifted(p.shift.0, p.shift.1)) }
    }
}

impl Cells for Book<'_> {
    fn get(&self, sheet: usize, r: u32, c: u32) -> V {
        if self.pending.get(sheet).is_some_and(|p| p.contains_key(&(r, c))) {
            return self.result(sheet, r, c);
        }
        V::from_value(self.values.get(sheet).and_then(|s| s.get(&(r, c))).and_then(|c| c.value.as_ref()))
    }
    fn sheet_index(&self, name: &str) -> Option<usize> {
        self.names.iter().position(|n| n.eq_ignore_ascii_case(name))
    }
    fn extent(&self, sheet: usize) -> (u32, u32) {
        self.extents.get(sheet).copied().unwrap_or((0, 0))
    }
}
