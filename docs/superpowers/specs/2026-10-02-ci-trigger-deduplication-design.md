# CI trigger deduplication

## Goal

Run the Linux and Windows CI matrix once for each pull-request update while retaining validation after changes reach `main`.

## Design

The CI workflow continues to run for every `pull_request`. Its `push` trigger is limited to the `main` branch, so a push to a feature branch with an open pull request no longer starts an equivalent second matrix.

A workflow-level concurrency group is keyed by the workflow and pull-request number when present, otherwise by the ref. Starting a newer run for the same pull request or branch cancels its obsolete in-progress run. Linux and Windows remain independent matrix jobs in the current run.

## Verification

Validate the workflow YAML, then confirm a pull-request update creates two jobs and a merge or direct push to `main` creates two more jobs. Confirm a newer push cancels the previous in-progress run for that pull request.
