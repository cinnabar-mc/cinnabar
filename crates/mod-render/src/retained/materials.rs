use super::*;
impl Store {
    /// Validates one procedural fragment after checking owner authority and compile quota.
    pub fn create_material(
        &mut self,
        source: &str,
        permissions: DrawingPermissions,
    ) -> Result<Handle, DrawError> {
        if !(permissions.world_draw && permissions.render_material) {
            return Err(error(ErrorCode::Denied, "material authority"));
        }
        if self.material_compiles >= mod_api::MAX_MATERIAL_COMPILES_PER_CALLBACK {
            return Err(error(ErrorCode::BudgetExceeded, "material compiles"));
        }
        self.material_compiles += 1;
        let shader = crate::material::compose(source)
            .map_err(|_| error(ErrorCode::Invalid, "material shader"))?;
        let material = Arc::new(crate::material::Material {
            source: source.into(),
            shader: shader.into(),
        });
        let handle = self.token()?;
        self.edit(source.len(), |resources| {
            resources.materials.insert(handle, material);
            resources.material_revision += 1;
            Ok(())
        })?;
        Ok(handle)
    }
}
