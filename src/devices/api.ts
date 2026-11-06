import { invoke } from "@tauri-apps/api/core";

// Mirrors src-tauri/src/devices.rs. Nothing is cached on the backend: call a list again to refresh.

export interface AudioDevice {
  name: string;
  outputChannels: number;
  /** Standard rates the device supports. */
  sampleRates: number[];
  isDefault: boolean;
}

/** An audio driver (API) of this system: ALSA, PipeWire, JACK, WASAPI, CoreAudio, ... The default comes first. */
export interface AudioHost {
  name: string;
  isDefault: boolean;
}

export interface MidiPort {
  name: string;
}

export function listAudioHosts(): Promise<AudioHost[]> {
  return invoke<AudioHost[]>("devices_list_hosts");
}

/** The devices of `driver`; `undefined` is the system's default driver. */
export function listAudioDevices(driver?: string): Promise<AudioDevice[]> {
  return invoke<AudioDevice[]>("devices_list_audio", { driver });
}

export function listMidiPorts(): Promise<MidiPort[]> {
  return invoke<MidiPort[]>("devices_list_midi");
}

/** Resolves a project's stored name hint; `null` means the device is not connected. */
export function findAudioDevice(name: string, driver?: string): Promise<AudioDevice | null> {
  return invoke<AudioDevice | null>("devices_find_audio", { name, driver });
}

/** Resolves a project's stored name hint, the `index`-th port of that name; `null` means it is not connected. */
export function findMidiPort(name: string, index = 0): Promise<MidiPort | null> {
  return invoke<MidiPort | null>("devices_find_midi", { name, index });
}
