//! External reads needed to reuse a pure scratch evaluation without running its bytecode.
use super::*;

#[derive(Debug)]
struct QueryRead {
    symbol: u32,
    /// Range of this call's arguments in [`ObservedInputs::arguments`].
    arguments: std::ops::Range<usize>,
    value: MolangValue,
}

/// The executed branch's input reads, excluding values the expression wrote itself.
///
/// Every list keeps its storage across [`Self::reset`], so re-recording an expression that
/// misses its cache each frame allocates nothing once warm.
#[derive(Debug)]
pub(crate) struct ObservedInputs {
    /// Inherited slots read before any write, in ascending symbol order.
    variables: Vec<(u32, Option<MolangValue>)>,
    /// Written symbols in ascending order.
    written: Vec<u32>,
    undo: Vec<(Place, Option<MolangValue>)>,
    temporary_checkpoint: usize,
    queries: Vec<QueryRead>,
    /// Arguments of every recorded query call, in call order.
    arguments: Vec<MolangValue>,
    random: u64,
}

impl ObservedInputs {
    /// Retains the initial random seed because scratch expressions reset it for each owner.
    pub(crate) fn new(variables: &MolangVariables) -> Self {
        Self {
            variables: Vec::new(),
            written: Vec::new(),
            undo: Vec::new(),
            temporary_checkpoint: variables.temporary_checkpoint(),
            queries: Vec::new(),
            arguments: Vec::new(),
            random: variables.random_state(),
        }
    }

    /// Starts a new recording against `variables`, keeping the lists' storage.
    pub(crate) fn reset(&mut self, variables: &MolangVariables) {
        self.variables.clear();
        self.written.clear();
        self.undo.clear();
        self.temporary_checkpoint = variables.temporary_checkpoint();
        self.queries.clear();
        self.arguments.clear();
        self.random = variables.random_state();
    }

    /// Charges each validated input and query argument; exhaustion leaves the result unresolved.
    pub(crate) fn matches(
        &self,
        evaluator: &Evaluator<'_>,
        variables: &MolangVariables,
        operations_left: &mut usize,
    ) -> Option<bool> {
        charge(operations_left, 1)?;
        if self.random != variables.random_state() {
            return Some(false);
        }
        for (symbol, expected) in &self.variables {
            let symbol = *symbol;
            charge(operations_left, 1)?;
            let Some(Place::Variable(slot)) = evaluator.layout.place(symbol) else {
                return Some(false);
            };
            if !same_value(variables.variable(slot).as_ref(), expected.as_ref()) {
                return Some(false);
            }
        }
        for read in &self.queries {
            let arguments = &self.arguments[read.arguments.clone()];
            charge(operations_left, 1 + arguments.len())?;
            if !same_value(
                Some(&evaluator.query(read.symbol, arguments)),
                Some(&read.value),
            ) {
                return Some(false);
            }
        }
        Some(true)
    }

    /// Counts stored values so the owner-selection cache has a shared memory bound.
    pub(crate) fn weight(&self) -> usize {
        1 + self.variables.len()
            + self.written.len()
            + self
                .queries
                .iter()
                .map(|read| 1 + read.arguments.len())
                .sum::<usize>()
    }

    /// Captures only inherited persistent slots read before this expression assigns them.
    pub(super) fn variable(&mut self, symbol: u32, place: Place, variables: &MolangVariables) {
        if let Place::Variable(slot) = place
            && self.written.binary_search(&symbol).is_err()
            && let Err(index) = self
                .variables
                .binary_search_by_key(&symbol, |(symbol, _)| *symbol)
        {
            self.variables
                .insert(index, (symbol, variables.variable(slot)));
        }
    }

    /// Records each slot's first pre-write value and excludes later reads from inherited inputs.
    pub(super) fn written(&mut self, symbol: u32, place: Place, variables: &MolangVariables) {
        if let Err(index) = self.written.binary_search(&symbol) {
            self.written.insert(index, symbol);
            #[cfg(test)]
            super::selection_work::record(|work| work.saved += 1);
            self.undo.push((place, variables.read(place)));
        }
    }

    /// Restores the selection prefix after success or failure, including clean temps and RNG.
    pub(crate) fn restore(&mut self, variables: &mut MolangVariables) {
        variables.restore(self.undo.drain(..), self.temporary_checkpoint);
        variables.restore_random(self.random);
    }

    /// Records the exact arguments and value of one query along the executed branch.
    pub(super) fn query(&mut self, symbol: u32, arguments: &[MolangValue], value: &MolangValue) {
        let start = self.arguments.len();
        self.arguments.extend_from_slice(arguments);
        self.queries.push(QueryRead {
            symbol,
            arguments: start..self.arguments.len(),
            value: value.clone(),
        });
    }
}

/// Reserves validation work before it runs, consuming the remainder on exhaustion.
fn charge(operations_left: &mut usize, work: usize) -> Option<()> {
    let remaining = operations_left.checked_sub(work);
    *operations_left = remaining.unwrap_or(0);
    remaining.map(|_| ())
}

/// Exact input identity distinguishes signed zero and retains stable NaN payloads.
fn same_value(left: Option<&MolangValue>, right: Option<&MolangValue>) -> bool {
    match (left, right) {
        (Some(MolangValue::Number(left)), Some(MolangValue::Number(right))) => {
            left.to_bits() == right.to_bits()
        }
        _ => left == right,
    }
}
