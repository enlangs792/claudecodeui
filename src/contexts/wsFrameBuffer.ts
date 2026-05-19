export type WsFrame = { seq: number; message: any };

export const appendInboundFrame = (
  frames: WsFrame[],
  frame: WsFrame,
  maxFrames = 4096,
): WsFrame[] => {
  frames.push(frame);
  if (frames.length <= maxFrames) {
    return frames;
  }
  return frames.slice(-maxFrames);
};

export const collectFramesSince = (frames: WsFrame[], lastSeq: number): WsFrame[] => {
  const since = Number.isFinite(lastSeq) && lastSeq > 0 ? lastSeq : 0;
  return frames.filter((frame) => frame.seq > since);
};
