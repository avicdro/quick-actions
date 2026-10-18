use enigo::{Direction, Enigo, Key, Keyboard as EnigoKeyboard, Settings};
use std::thread;
use std::time::Duration;

use crate::error::{AppError, Result};

/// Simula pulsaciones de teclado usando `enigo`.
pub struct KeySimulator;

impl KeySimulator {
    fn enigo() -> Result<Enigo> {
        Enigo::new(&Settings::default())
            .map_err(|e| AppError::Keyboard(format!("Error creando Enigo: {e}")))
    }

    /// Libera todas las teclas modificadoras que puedan estar presionadas.
    ///
    /// CRITICO: despues de que el hook consume la tecla mapeada, el
    /// modificador sigue fisicamente presionado (Alt o CapsLock, segun
    /// config). Si simulamos Ctrl+C mientras esta presionado, el sistema
    /// ve terciopelos corridos (Alt+Ctrl+C) que no copian correctamente,
    /// y el texto queda vacio (causa del bug "selecciona todo").
    fn release_modifiers() -> Result<()> {
        // Esperar a que el usuario suelte fisicamente el modificador
        // configurado. Hasta 2 segundos (40 x 50ms).
        #[cfg(windows)]
        {
            use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;

            let watch: &[i32] = match crate::hotkeys::modifier() {
                crate::hotkeys::Modifier::Alt => &[0x12],      // Alt
                crate::hotkeys::Modifier::CapsLock => &[0x14], // CapsLock
            };
            for _ in 0..40 {
                let down = watch
                    .iter()
                    .any(|&vk| unsafe { (GetAsyncKeyState(vk) as u16) & 0x8000 != 0 });
                if !down {
                    break;
                }
                thread::sleep(Duration::from_millis(50));
            }
        }

        // Adicionalmente, simular la liberacion de todos los modificadores
        // por si el estado del teclado no se actualizo.
        let mut enigo = Self::enigo()?;
        for key in [Key::Alt, Key::Control, Key::Shift, Key::Meta] {
            let _ = EnigoKeyboard::key(&mut enigo, key, Direction::Release);
        }
        thread::sleep(Duration::from_millis(50));
        Ok(())
    }

    /// Simula `Ctrl+C` (copiar seleccion).
    pub fn copy() -> Result<()> {
        Self::release_modifiers()?;
        let mut enigo = Self::enigo()?;
        EnigoKeyboard::key(&mut enigo, Key::Control, Direction::Press)
            .map_err(|e| AppError::Keyboard(format!("Error teclado: {e}")))?;
        EnigoKeyboard::key(&mut enigo, Key::Unicode('c'), Direction::Click)
            .map_err(|e| AppError::Keyboard(format!("Error teclado: {e}")))?;
        EnigoKeyboard::key(&mut enigo, Key::Control, Direction::Release)
            .map_err(|e| AppError::Keyboard(format!("Error teclado: {e}")))?;
        thread::sleep(Duration::from_millis(150));
        Ok(())
    }

    /// Simula `Ctrl+V` (pegar).
    pub fn paste() -> Result<()> {
        Self::release_modifiers()?;
        let mut enigo = Self::enigo()?;
        EnigoKeyboard::key(&mut enigo, Key::Control, Direction::Press)
            .map_err(|e| AppError::Keyboard(format!("Error teclado: {e}")))?;
        EnigoKeyboard::key(&mut enigo, Key::Unicode('v'), Direction::Click)
            .map_err(|e| AppError::Keyboard(format!("Error teclado: {e}")))?;
        EnigoKeyboard::key(&mut enigo, Key::Control, Direction::Release)
            .map_err(|e| AppError::Keyboard(format!("Error teclado: {e}")))?;
        thread::sleep(Duration::from_millis(50));
        Ok(())
    }

    /// Simula `Ctrl+A` (seleccionar todo).
    pub fn select_all() -> Result<()> {
        Self::release_modifiers()?;
        let mut enigo = Self::enigo()?;
        EnigoKeyboard::key(&mut enigo, Key::Control, Direction::Press)
            .map_err(|e| AppError::Keyboard(format!("Error teclado: {e}")))?;
        EnigoKeyboard::key(&mut enigo, Key::Unicode('a'), Direction::Click)
            .map_err(|e| AppError::Keyboard(format!("Error teclado: {e}")))?;
        EnigoKeyboard::key(&mut enigo, Key::Control, Direction::Release)
            .map_err(|e| AppError::Keyboard(format!("Error teclado: {e}")))?;
        thread::sleep(Duration::from_millis(50));
        Ok(())
    }

    /// Simula `End` (ir al final de la linea).
    pub fn end() -> Result<()> {
        let mut enigo = Self::enigo()?;
        EnigoKeyboard::key(&mut enigo, Key::End, Direction::Click)
            .map_err(|e| AppError::Keyboard(format!("Error teclado: {e}")))?;
        thread::sleep(Duration::from_millis(50));
        Ok(())
    }

    /// Simula `Shift+Home` (seleccionar hasta el inicio de la linea).
    pub fn shift_home() -> Result<()> {
        let mut enigo = Self::enigo()?;
        EnigoKeyboard::key(&mut enigo, Key::Shift, Direction::Press)
            .map_err(|e| AppError::Keyboard(format!("Error teclado: {e}")))?;
        EnigoKeyboard::key(&mut enigo, Key::Home, Direction::Click)
            .map_err(|e| AppError::Keyboard(format!("Error teclado: {e}")))?;
        EnigoKeyboard::key(&mut enigo, Key::Shift, Direction::Release)
            .map_err(|e| AppError::Keyboard(format!("Error teclado: {e}")))?;
        thread::sleep(Duration::from_millis(50));
        Ok(())
    }

    /// Simula `Shift+End` (seleccionar hasta el final de la linea).
    pub fn shift_end() -> Result<()> {
        let mut enigo = Self::enigo()?;
        EnigoKeyboard::key(&mut enigo, Key::Shift, Direction::Press)
            .map_err(|e| AppError::Keyboard(format!("Error teclado: {e}")))?;
        EnigoKeyboard::key(&mut enigo, Key::End, Direction::Click)
            .map_err(|e| AppError::Keyboard(format!("Error teclado: {e}")))?;
        EnigoKeyboard::key(&mut enigo, Key::Shift, Direction::Release)
            .map_err(|e| AppError::Keyboard(format!("Error teclado: {e}")))?;
        thread::sleep(Duration::from_millis(50));
        Ok(())
    }
}
