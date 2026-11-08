use enigo::{Direction, Enigo, Key, Keyboard as EnigoKeyboard, Settings};
use std::thread;
use std::time::Duration;

use crate::error::{AppError, Result};
use crate::hotkeys::Modifier;

/// Simula `Ctrl+<letra>` por TECLA VIRTUAL Win32 (VK_C/V/A), no por
/// caracter Unicode/scancode.
///
/// Diagnostico v0.3.0-hotfix: enigo traduce `Key::Unicode('c')` via
/// `VkKeyScanW` (layout activo) + scancode. El resultado NO disparaba el
/// acelerador Ctrl+C de las apps (notepad incluido: portapapeles vacio,
/// "No hay texto para corregir"). Con VK explicito, SendInput entrega el
/// atajo exacto que cualquier app reconoce, independiente del layout.
#[cfg(windows)]
fn ctrl_vk(vk: u16) -> Result<()> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VIRTUAL_KEY,
    };

    const VK_CONTROL: u16 = 0x11;
    let key = |vk: u16, up: bool| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                wScan: 0,
                dwFlags: if up {
                    KEYEVENTF_KEYUP
                } else {
                    Default::default()
                },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let seq = [
        key(VK_CONTROL, false),
        key(vk, false),
        key(vk, true),
        key(VK_CONTROL, true),
    ];
    let sent = unsafe { SendInput(&seq, std::mem::size_of::<INPUT>() as i32) };
    if sent as usize != seq.len() {
        return Err(AppError::Keyboard(format!(
            "SendInput Ctrl+VK(0x{vk:02X}): solo {sent}/{} eventos entregados",
            seq.len()
        )));
    }
    Ok(())
}

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
    ///
    /// Fase 1.3: el modificador APLICADO (capturado por el hook al arrancar)
    /// llega por parametro; ya no hay static `MODIFIER` que consultar.
    fn release_modifiers(modifier: Modifier) -> Result<()> {
        // Esperar a que el usuario suelte fisicamente el modificador
        // configurado. Hasta 2 segundos (40 x 50ms).
        #[cfg(windows)]
        {
            use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;

            let watch: &[i32] = match modifier {
                Modifier::Alt => &[0x12],      // Alt
                Modifier::CapsLock => &[0x14], // CapsLock
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
    pub fn copy(modifier: Modifier) -> Result<()> {
        Self::release_modifiers(modifier)?;
        // v0.3.0-hotfix: VK directo, no enigo Unicode/scancode (ver ctrl_vk).
        ctrl_vk(0x43)?; // VK_C
        thread::sleep(Duration::from_millis(150));
        Ok(())
    }

    /// Simula `Ctrl+V` (pegar).
    pub fn paste(modifier: Modifier) -> Result<()> {
        Self::release_modifiers(modifier)?;
        ctrl_vk(0x56)?; // VK_V
        thread::sleep(Duration::from_millis(50));
        Ok(())
    }

    /// Simula `Ctrl+A` (seleccionar todo).
    pub fn select_all(modifier: Modifier) -> Result<()> {
        Self::release_modifiers(modifier)?;
        ctrl_vk(0x41)?; // VK_A
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
