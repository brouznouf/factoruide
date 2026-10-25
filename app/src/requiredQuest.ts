/** Convert the player's quest entry to the request's ID or exact-name choice. */
export function requiredQuest(input: string): number | string {
  const value = input.trim();
  return /^[1-9]\d*$/.test(value) ? Number(value) : value;
}
