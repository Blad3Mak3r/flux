import { errorMessage } from "../project/api";
import { sayError } from "./ui";

/** Shows what went wrong in the error dialog: the one way a failed command is reported. */
export const fail = (err: unknown): void => sayError(errorMessage(err));
