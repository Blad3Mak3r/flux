# DMX lighting

A **DMX track** drives lights over the network, locked to the timeline. Its items are **scenes**: at its time a scene
fades the lights to the values it holds, the way a light cue does on a lighting console or in QLab. FLUX sends
**Art-Net** or **sACN (E1.31)**; it is the sender and never reads DMX from the network (that is the
[Art-Net control input](remote-control.md), a different thing).

## Set up a destination

1. Open Preferences > Outputs > DMX and choose **Add DMX destination**.
2. Pick the protocol, Art-Net or sACN, and the host: the address of a node or a console, or `255.255.255.255` to
   broadcast on the local network (Art-Net nodes usually listen to broadcasts).
3. The lamp next to it says whether it opened while the show plays. A host that does not resolve never stops the show.

Destinations belong to the project, like OSC destinations, so the project keeps them wherever it is opened.

## Add scenes

- Choose **Add DMX track** in the track menu, or double click on the empty part of a DMX lane to add a scene there.
- A scene is a line on the lane with a tag, and a bar as long as its fade. Drag it to move it; the Inspector edits it.
- In the Inspector set the **destination** of the track, the scene's **name**, its **fade** (a timecode length; all zeros
  is at once) and **Release the other channels**.
- Add the **channels** the scene sets: a universe, a channel and a value 0 to 255. The backend keeps them sorted and
  refuses a channel set twice or out of range.

Universes count from **0**, as Art-Net numbers them (the first universe of many consoles, "universe 1", is 0 here); sACN
sends the same number plus one. A universe has channels 1 to 512.

## What the lights do

- A scene fades from what the lights show when it starts to the scene's values, in a straight line over its fade.
- Channels a scene does not list **keep their value** (tracking), unless the scene releases the others: then they fade to 0.
- A scene that starts during a fade takes over from where each channel had got to.
- Before the first scene every channel is 0, and a channel no scene sets stays at 0.
- Several DMX tracks on one destination merge channel by channel: the highest value wins.

## Playing, locating and stopping

The lights always match the position: locating, starting from the middle or jumping to a cue in Live shows exactly the
state of that point, including a fade in progress. **Stop and pause hold the last frame**; the lights do not go dark.
FLUX repeats the frame once a second so a node that lost it recovers. To turn the lights off at the end of a show, end
it with a scene that sets channels to 0, or releases the others. While Edit auditions on a preview device the DMX
tracks send nothing, like the other cue tracks.

## In the Live view and the show check

Each scene is a cue of kind DMX in the Live view, with its channel count and the time to it, and GO jumps to it like
any other cue. The show check reports a DMX track that has no destination or whose destination has an invalid host.

The format of the track is in [the project file format](fxtc-format.md) and the threads in
[timecode signals](timecode-signals.md).
