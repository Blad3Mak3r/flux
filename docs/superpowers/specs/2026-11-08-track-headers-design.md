# Track header spacing

Approved direction: preserve the Bench Instrument identity and give track identity,
audio routing and level controls separate rows. Headers widen from 208 to 248px;
audio and operator lanes grow from 96 to 120px. Other lanes remain 64px tall.

The first row holds the kind icon, editable name and fixed-size mute button. The
second row shows a recessed routing button for audio/operator tracks, or the kind
label for other tracks. The translated muted tag sits on this second row so it
does not compete with the name. Routing includes an output icon, truncating name
and inspector chevron, with the full destination in its tooltip and accessible name.
Fader and meter follow in aligned rows with consistent spacing.

Reuse the existing components, translations, backend-provided routing state and
selection action. No new editing logic or dependencies. Shared lane heights keep
headers, clips, scrolling and reorder feedback aligned. Check TypeScript, frontend
tests, the layout detector and production build. Visual review must cover long
names, muted tracks, English/Spanish and the 960px minimum window.
