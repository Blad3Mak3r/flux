# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

## Users

Lighting technicians running shows who need to inspect and maintain a local bridge between Art-Net/DMX from localhost and an ENTTEC Open DMX USB interface.

## Product Purpose

Flux relays Art-Net `ArtDmx` to physical DMX through an ENTTEC Open DMX USB interface. It lets the technician confirm the route is healthy, identify an interruption quickly, and adjust configuration without interrupting their work.

## Positioning

It is a local, deliberately direct bridge for an Art-Net-to-ENTTEC Open DMX route, not a generic lighting-control framework.

## Operating Context

It is used during setup and live shows. Input is usually localhost and output is a USB interface; incoming signal, device, and transmission may change state during operation.

## Capabilities and Constraints

- Receives Art-Net `ArtDmx`, filters a Port-Address, and sends DMX through FTDI D2XX to an ENTTEC Open DMX interface.
- Keeps the last valid frame and reconnects automatically after device or output errors.
- The main view always shows bridge status. Controls and channel detail may live in secondary views.
- Configuration includes listen address, universe, rate, channel count, and FTDI device.

## Evidence on Hand

The current implementation and states live in `ui/src/main.tsx`; operational description and usage examples are in `README.md`. The repository contains brand assets in `assets/flux.*`.

## Product Principles

- Bridge operating status is visible at a glance.
- Failure response must be quick and unambiguous.
- Detail is revealed where it aids diagnosis without displacing the primary situation.
- The simplicity of one reliable route takes priority over generic abstraction.
