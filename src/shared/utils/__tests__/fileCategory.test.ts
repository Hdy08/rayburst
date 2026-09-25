import { describe, it, expect } from 'vitest'
import { validateCategoryUrlPatterns } from '../fileCategory'

describe('validateCategoryUrlPatterns', () => {
  it('accepts valid wildcard URL rules', () => {
    expect(validateCategoryUrlPatterns(['*://*.example.com/logs/*'], 'wildcard')).toBeUndefined()
  })

  it('reports the first invalid regex URL rule line', () => {
    expect(validateCategoryUrlPatterns(['^https://reports\\.example\\.com/.+$', '^https://(.+'], 'regex')).toEqual({
      line: 2,
      reason: 'invalid-regex',
    })
  })

  it('reports overlong URL rules instead of dropping them silently', () => {
    expect(validateCategoryUrlPatterns(['a'.repeat(513)], 'wildcard')).toEqual({
      line: 1,
      reason: 'too-long',
    })
  })
})
