// Where Help and About send people. The pages open in the system browser through the opener plugin.

export const DOCS_URL = "https://blad3mak3r.github.io/flux/docs/";
export const REPO_URL = "https://github.com/Blad3Mak3r/flux";
export const ISSUES_URL = `${REPO_URL}/issues/new`;
/** The GitHub Release of a version, whose notes say what it changes. */
export const releaseUrl = (version: string) => `${REPO_URL}/releases/tag/v${version}`;
