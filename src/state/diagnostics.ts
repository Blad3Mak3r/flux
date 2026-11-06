import { exportDiagnostics, openLogDir } from "../diagnostics/api";
import { errorMessage } from "../project/api";
import { say, sayError } from "./ui";

export const openLogFolder = () => openLogDir().catch((err) => sayError(errorMessage(err)));

export async function exportDiagnosticsFile() {
  try {
    if ((await exportDiagnostics()) !== null) say("status.diagnosticsExported");
  } catch (err) {
    sayError(errorMessage(err));
  }
}
