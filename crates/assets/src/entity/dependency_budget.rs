use std::mem::size_of;

use super::EntityDependency;

/// Checked estimate of the owned storage retained for catalog dependencies.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EntityDependencyStorageBudget {
    used_bytes: usize,
    limit_bytes: usize,
}

impl EntityDependencyStorageBudget {
    /// Creates a budget, allowing small limits for boundary tests.
    #[must_use]
    pub const fn new(limit_bytes: usize) -> Self {
        Self {
            used_bytes: 0,
            limit_bytes,
        }
    }

    /// Charges retained slice entries and identifier bytes, excluding allocator overhead.
    pub fn charge_many(
        &mut self,
        count: usize,
        identifier_bytes: usize,
    ) -> Result<(), &'static str> {
        let entries = count
            .checked_mul(size_of::<EntityDependency>())
            .ok_or("dependency entry accounting overflow")?;
        let charge = entries
            .checked_add(identifier_bytes)
            .ok_or("dependency identifier accounting overflow")?;
        let used_bytes = self
            .used_bytes
            .checked_add(charge)
            .ok_or("dependency total accounting overflow")?;
        if used_bytes > self.limit_bytes {
            return Err("dependency storage exceeds budget");
        }
        self.used_bytes = used_bytes;
        Ok(())
    }

    /// Charges one retained dependency and its owned identifier bytes.
    pub fn charge(&mut self, identifier_bytes: usize) -> Result<(), &'static str> {
        self.charge_many(1, identifier_bytes)
    }

    /// Returns the successfully charged storage estimate.
    #[must_use]
    pub const fn used_bytes(self) -> usize {
        self.used_bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dependency_storage_budget_checks_exact_boundary_and_overflow() {
        let entry_size = size_of::<EntityDependency>();
        let mut budget = EntityDependencyStorageBudget::new(entry_size + 3);
        assert!(budget.charge(3).is_ok());
        assert_eq!(budget.used_bytes(), entry_size + 3);
        assert!(budget.charge(0).is_err());
        assert_eq!(budget.used_bytes(), entry_size + 3);

        let mut overflow = EntityDependencyStorageBudget::new(usize::MAX);
        assert!(overflow.charge(usize::MAX).is_err());
        assert!(overflow.charge_many(usize::MAX, 0).is_err());
        assert_eq!(overflow.used_bytes(), 0);
        overflow.charge_many(0, usize::MAX).unwrap();
        assert!(overflow.charge(0).is_err());
        assert_eq!(overflow.used_bytes(), usize::MAX);
    }
}
