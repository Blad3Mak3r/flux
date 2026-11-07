/** Whether output number `index` is lost, in a mask the backend sends with one bit per output. Only the first 32 have one. */
export const isLost = (mask: number, index: number): boolean => index < 32 && ((mask >>> index) & 1) === 1;
