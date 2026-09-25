/** Category editing and native directory resolution. */
import { invoke } from '@tauri-apps/api/core'
import type { FileCategory } from '@shared/types'

const MAX_URL_PATTERN_LENGTH = 512
export interface CategoryMatchContext {
  urls?: readonly string[]
}
export interface CategoryFileCandidate {
  path: string
  urls?: readonly string[]
}
export interface CategoryUrlPatternValidationError {
  line: number
  reason: 'invalid-regex' | 'too-long'
}
function normalizeExtension(value: string): string {
  return value.trim().toLowerCase().replace(/^\./, '')
}

export function normalizeCategoryUrlPatterns(patterns: unknown): string[] {
  if (!Array.isArray(patterns)) return []
  const result: string[] = []
  const seen = new Set<string>()

  for (const value of patterns) {
    if (typeof value !== 'string') continue
    const pattern = value.trim()
    if (!pattern || pattern.length > MAX_URL_PATTERN_LENGTH || seen.has(pattern)) continue
    seen.add(pattern)
    result.push(pattern)
  }

  return result
}

export function validateCategoryUrlPatterns(
  patterns: readonly string[],
  mode: FileCategory['urlPatternMode'],
): CategoryUrlPatternValidationError | undefined {
  const normalizedMode = mode === 'regex' ? 'regex' : 'wildcard'

  for (let index = 0; index < patterns.length; index += 1) {
    const pattern = patterns[index]?.trim() ?? ''
    if (!pattern) continue
    if (pattern.length > MAX_URL_PATTERN_LENGTH) return { line: index + 1, reason: 'too-long' }

    try {
      if (normalizedMode === 'regex') {
        new RegExp(pattern, 'i')
      }
    } catch {
      return {
        line: index + 1,
        reason: 'invalid-regex',
      }
    }
  }

  return undefined
}

export function normalizeFileCategory(category: FileCategory): FileCategory {
  const mode = category.urlPatternMode === 'regex' ? 'regex' : 'wildcard'
  return {
    ...category,
    extensions: Array.from(new Set(category.extensions.map(normalizeExtension).filter(Boolean))),
    urlPatterns: normalizeCategoryUrlPatterns(category.urlPatterns),
    urlPatternMode: mode,
  }
}

export async function resolveFileSetCategory(
  files: readonly CategoryFileCandidate[],
  categories: FileCategory[],
  baseDir: string,
  context?: CategoryMatchContext,
): Promise<FileCategory | undefined> {
  const category = await invoke<FileCategory | null>('resolve_file_category', {
    candidates: files.map((file) => ({ path: file.path, urls: [...(file.urls ?? []), ...(context?.urls ?? [])] })),
    categories,
    baseDir,
  })
  return category ?? undefined
}

export function resolveDownloadCategory(
  value: string,
  categories: FileCategory[],
  baseDir: string,
  context?: CategoryMatchContext,
): Promise<FileCategory | undefined> {
  return resolveFileSetCategory([{ path: value }], categories, baseDir, context)
}

export async function resolveDownloadDir(
  value: string,
  baseDir: string,
  enabled: boolean,
  categories: FileCategory[],
  context?: CategoryMatchContext,
): Promise<string> {
  if (!enabled) return baseDir
  return (await resolveDownloadCategory(value, categories, baseDir, context))?.directory ?? baseDir
}
