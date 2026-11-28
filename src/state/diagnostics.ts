import { fail } from "./fail";
import { exportDiagnostics, openLogDir } from "../diagnostics/api";
import { say } from "./ui";

export const openLogFolder = () => openLogDir().catch((err) => fail(err));

export async function exportDiagnosticsFile() {
  try {
    if ((await exportDiagnostics()) !== null) say("status.diagnosticsExported");
  } catch (err) {
    fail(err);
  }
}
