use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::error::Result;

/// Entrada individual del historial de correcciones.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct HistoryEntry {
    /// Texto original antes de la correccion.
    pub original: String,
    /// Texto corregido por Ollama.
    pub corregido: String,
    /// Fecha y hora de la correccion (formato: dd/MM HH:mm:ss).
    pub fecha: String,
}

/// Historial de correcciones en memoria con persistencia JSON.
pub struct History {
    entries: Vec<HistoryEntry>,
    max_entries: usize,
    path: PathBuf,
}

impl History {
    /// Crea o carga el historial desde disco.
    ///
    /// Si el archivo no existe, empieza vacio.
    pub fn load_or_create<P: AsRef<Path>>(path: P, max_entries: usize) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let entries = if path.exists() {
            let content = std::fs::read_to_string(&path)?;
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            Vec::new()
        };

        Ok(Self { entries, max_entries, path })
    }

    /// Anade una nueva entrada al historial (al inicio).
    pub fn push(&mut self, original: String, corregido: String) {
        let now = chrono::Local::now();
        let fecha = now.format("%d/%m %H:%M:%S").to_string();

        self.entries.insert(0, HistoryEntry { original, corregido, fecha });

        // Truncar si excede el maximo
        while self.entries.len() > self.max_entries {
            self.entries.pop();
        }
    }

    /// Obtiene todas las entradas del historial.
    pub fn entries(&self) -> &[HistoryEntry] {
        &self.entries
    }

    /// Obtiene una entrada por indice (1-based, como en AHK).
    pub fn get(&self, index: usize) -> Option<&HistoryEntry> {
        if index == 0 {
            return None;
        }
        self.entries.get(index - 1)
    }

    /// Guarda el historial a disco de forma atomica.
    ///
    /// Escribe a un archivo temporal y luego hace rename.
    pub fn save(&self) -> Result<()>
 {
        if self.entries.is_empty() {
            return Ok(());
        }

        let json = serde_json::to_string_pretty(&self.entries)?;

        // Escribir a archivo temporal
        let temp_path = self.path.with_extension("tmp");
        std::fs::write(&temp_path, json)?;

        // Rename atomico
        std::fs::rename(&temp_path, &self.path)?;

        Ok(())
    }

    /// Crea un historial vacio (sin path de guardado).
    pub fn empty(max_entries: usize) -> Self {
        Self {
            entries: Vec::new(),
            max_entries,
            path: PathBuf::from("history.json"),
        }
    }

    /// Devuelve la ruta del archivo de historial.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_push_and_get() {
        let mut h = History::empty(10);
        assert!(h.entries().is_empty());

        h.push("hola mundo".to_string(), "Hola Mundo".to_string());
        assert_eq!(h.entries().len(), 1);
        assert_eq!(h.entries()[0].original, "hola mundo");
        assert_eq!(h.entries()[0].corregido, "Hola Mundo");

        // get es 1-based
        assert!(h.get(0).is_none());
        assert!(h.get(1).is_some());
        assert!(h.get(2).is_none());
    }

    #[test]
    fn history_max_entries_truncates() {
        let mut h = History::empty(3);
        h.push("a".to_string(), "A".to_string());
        h.push("b".to_string(), "B".to_string());
        h.push("c".to_string(), "C".to_string());
        h.push("d".to_string(), "D".to_string());

        assert_eq!(h.entries().len(), 3);
        // El mas reciente va primero
        assert_eq!(h.entries()[0].original, "d");
        // El mas antiguo (a) se elimina
        assert_eq!(h.entries()[2].original, "b");
    }

    #[test]
    fn history_empty_is_empty() {
        let h = History::empty(50);
        assert!(h.entries().is_empty());
    }
}
