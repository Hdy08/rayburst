import { describe, expect, it } from 'vitest'
import speedometerSource from '../Speedometer.vue?raw'

describe('Speedometer appearance', () => {
  it('keeps foreground opacity independent from the configured button opacity', () => {
    const speedometerRule = speedometerSource.match(/\.speedometer \{([\s\S]*?)\n\}/)?.[1]

    expect(speedometerSource).toContain('opacityPercentToCssPercent(')
    expect(speedometerSource).toContain("'--speedometer-opacity-percent'")
    expect(speedometerRule).toContain('background: color-mix')
    expect(speedometerRule).toContain('var(--speedometer-opacity-percent)')
    expect(speedometerRule).not.toMatch(/(^|\n)\s*opacity\s*:/)
  })
})
