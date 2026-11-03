import { invoke } from "@tauri-apps/api/core";

// Mirrors src-tauri/src/devices.rs. Nothing is cached on the backend: call a list again to refresh.

export interface AudioDevice {
  /** Audio API the device was found through (ALSA, JACK, WASAPI, CoreAudio, ...). */
  host: string;
  name: string;
  outputChannels: number;
  /** Standard rates the device supports. */
  sampleRates: number[];
  isDefault: boolean;
}

export interface MidiPort {
  name: string;
}

export function listAudioDevices(): Promise<AudioDevice[]> {
  return invoke<AudioDevice[]>("devices_list_audio");
}

export function listMidiPorts(): Promise<MidiPort[]> {
  return invoke<MidiPort[]>("devices_list_midi");
}

/** Resolves a project's stored name hint; `null` means the device is not connected. */
export function findAudioDevice(name: string): Promise<AudioDevice | null> {
  return invoke<AudioDevice | null>("devices_find_audio", { name });
}

/** Resolves a project's stored name hint; `null` means the port is not connected. */
export function findMidiPort(name: string): Promise<MidiPort | null> {
  return invoke<MidiPort | null>("devices_find_midi", { name });
}
