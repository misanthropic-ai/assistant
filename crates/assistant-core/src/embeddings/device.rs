use anyhow::Result;
use candle_core::Device;
use serde::{Deserialize, Serialize};

/// Device selection for ML models
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DevicePreference {
    /// Automatically select the best available device
    Auto,
    /// Prefer CUDA if available, fallback to CPU
    Cuda,
    /// Prefer Metal if available, fallback to CPU
    Metal,
    /// Always use CPU
    Cpu,
}

impl Default for DevicePreference {
    fn default() -> Self {
        DevicePreference::Auto
    }
}

/// Detect and return the best available device for inference
pub fn detect_best_device(preference: &DevicePreference) -> Result<Device> {
    match preference {
        DevicePreference::Auto => {
            // Try CUDA first
            #[cfg(feature = "cuda")]
            if candle_core::utils::cuda_is_available() {
                tracing::info!("Using CUDA device for embeddings");
                return Device::new_cuda(0);
            }
            
            // Try Metal on macOS
            #[cfg(all(target_os = "macos", feature = "metal"))]
            if candle_core::utils::metal_is_available() {
                tracing::info!("Using Metal device for embeddings");
                return Device::new_metal(0);
            }
            
            // Fallback to CPU
            tracing::info!("Using CPU device for embeddings");
            Ok(Device::Cpu)
        }
        
        DevicePreference::Cuda => {
            #[cfg(feature = "cuda")]
            if candle_core::utils::cuda_is_available() {
                tracing::info!("Using CUDA device for embeddings");
                return Device::new_cuda(0);
            }
            
            tracing::warn!("CUDA requested but not available, falling back to CPU");
            Ok(Device::Cpu)
        }
        
        DevicePreference::Metal => {
            #[cfg(all(target_os = "macos", feature = "metal"))]
            if candle_core::utils::metal_is_available() {
                tracing::info!("Using Metal device for embeddings");
                return Device::new_metal(0);
            }
            
            tracing::warn!("Metal requested but not available, falling back to CPU");
            Ok(Device::Cpu)
        }
        
        DevicePreference::Cpu => {
            tracing::info!("Using CPU device for embeddings");
            Ok(Device::Cpu)
        }
    }
}

/// Get device info as a string
pub fn get_device_info(device: &Device) -> String {
    match device {
        Device::Cpu => "CPU".to_string(),
        Device::Cuda(_) => "CUDA GPU".to_string(),
        Device::Metal(_) => "Metal Device".to_string(),
    }
}

/// Check if accelerated compute is available
pub fn has_accelerated_compute() -> bool {
    #[cfg(feature = "cuda")]
    if candle_core::utils::cuda_is_available() {
        return true;
    }
    
    #[cfg(all(target_os = "macos", feature = "metal"))]
    if candle_core::utils::metal_is_available() {
        return true;
    }
    
    false
}