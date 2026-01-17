//! Model selection and management.
//!
//! This module contains model-related methods for the App.
//! Similar to context/local.tsx model handling in the TS version.

use anyhow::Result;

use super::state::App;
use crate::provider;

/// Model-related methods for App
impl App {
    /// Set the current model
    pub async fn set_model(&mut self, provider_id: &str, model_id: &str) -> Result<()> {
        // Verify the model exists
        let model = provider::registry()
            .get_model(provider_id, model_id)
            .await
            .ok_or_else(|| anyhow::anyhow!("Model not found: {}/{}", provider_id, model_id))?;

        self.provider_id = provider_id.to_string();
        self.model_id = model_id.to_string();
        self.model_display = format!("{}/{}", provider_id, model.name);
        self.model_configured = true;
        self.close_dialog();

        // Save to session
        if let Some(session) = &mut self.session {
            let model_ref = crate::session::ModelRef {
                provider_id: provider_id.to_string(),
                model_id: model_id.to_string(),
            };
            if let Err(e) = session
                .set_model(&session.project_id.clone(), model_ref)
                .await
            {
                tracing::warn!("Failed to save model to session: {}", e);
            }
        }

        // Save last used model to global storage (fallback)
        let model_string = format!("{}/{}", provider_id, model_id);
        if let Err(e) = crate::storage::global()
            .write(&["state", "last_model"], &model_string)
            .await
        {
            tracing::warn!("Failed to save last used model: {}", e);
        }

        Ok(())
    }
}
