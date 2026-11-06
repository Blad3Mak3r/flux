/** Up to three initials from whitespace- and camelCase-delimited project-name words. */
export function projectInitials(name: string): string {
  return name
    .trim()
    .split(/\s+|(?<=[a-z])(?=[A-Z])/u)
    .slice(0, 3)
    .map((word) => word[0])
    .join("")
    .toLocaleUpperCase();
}
