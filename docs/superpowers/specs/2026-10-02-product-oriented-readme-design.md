# Product-oriented README

## Goal

Rewrite the English README so Flux is presented first as a focused desktop operating surface for an Art-Net-to-DMX bridge, rather than as a Rust implementation or a generic lighting framework.

## Audience and voice

The README addresses lighting technicians setting up or supervising a local Art-Net-to-ENTTEC Open DMX route. It uses concise operational language: what is visible, what requires action, and what continues running in the background. Technical implementation detail remains only where it changes installation, safe operation, or hardware expectations.

## Structure

1. A concise product introduction and route diagram.
2. An `Operate the bridge` section covering the permanent route view, route health states, settings modal, monitor, reconnect action, custom desktop window controls, and close-to-tray behavior.
3. A short MagicQ quick start with defaults and the LAN listen-address variation.
4. A compact CLI reference for headless operation, diagnostics, and explicit device selection.
5. A focused reliability and hardware-limits section: retained last valid frame, reconnection, Open DMX timing limitations, and show-testing recommendation.
6. Brief development, release, signed update, and product-roadmap sections.

## Content changes

Remove the broad “desktop dashboard” wording and the long version-by-version historical checklist. Describe the UI as an operational route console: the route map and output inspector are always visible; configuration is opened from the title strip; the channel monitor is on demand; and closing the window hides it to the tray while routing continues.

Keep the MagicQ setup, defaults, CLI options, build command, updater-signing policy, and hardware caveats, but shorten them to material needed by an operator or contributor. Do not make promises about unsupported protocols, devices, automation, or availability.

## Verification

1. Check commands, defaults, configuration fields, and tray behavior against the current implementation.
2. Confirm the README presents the desktop operational workflow before implementation detail.
3. Review headings for scanability and ensure all product claims are supported by the code or existing release configuration.
