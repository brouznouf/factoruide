/**
 * The name the guide form gives a guide by itself: the character's name (without its realm)
 * and the levels the guide covers, "Besmer 25-40". It follows the character and both levels
 * until the player types a name of their own.
 */
export function autoGuideName(character: string, fromLevel: number, toLevel: number): string {
  return `${character.split("-")[0]} ${fromLevel}-${Math.max(toLevel, fromLevel + 1)}`;
}
