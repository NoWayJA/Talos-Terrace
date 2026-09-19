import { describe, expect, it } from 'vitest';
import {
  LAYERS,
  LAYER_GAP,
  LAYER_HEIGHT,
  LAYER_IDS,
  ROW_HEIGHT,
  SINGLE_ROW_HEIGHTS,
  isLayerId,
  layerAtY,
  layerById,
  layerHeight,
  layerPosition,
} from './layers';

describe('layers', () => {
  it('stack from the trust boundary at the top to hardware at the bottom', () => {
    expect(LAYER_IDS[0]).toBe('trust');
    expect(LAYER_IDS[LAYER_IDS.length - 1]).toBe('hardware');
    expect(LAYERS.map((layer) => layer.id)).toEqual([...LAYER_IDS]);
  });

  it('give every layer an upstream documentation link', () => {
    for (const layer of LAYERS) {
      expect(layer.docs.url.startsWith('https://')).toBe(true);
      expect(layer.docs.label.length).toBeGreaterThan(0);
    }
  });

  it('place each band below the previous one without overlap', () => {
    for (let index = 1; index < LAYER_IDS.length; index += 1) {
      const above = LAYER_IDS[index - 1];
      const below = LAYER_IDS[index];
      if (!above || !below) throw new Error('unreachable');
      expect(layerPosition(below).y).toBeGreaterThanOrEqual(layerPosition(above).y + LAYER_HEIGHT);
    }
  });

  it('find the band under a y coordinate and nothing in the gaps', () => {
    expect(layerAtY(1)).toBe('trust');
    expect(layerAtY(layerPosition('kubernetes').y + 10)).toBe('kubernetes');
    expect(layerAtY(layerPosition('workloads').y + LAYER_HEIGHT + 1)).toBeNull();
    expect(layerAtY(-5)).toBeNull();
  });

  it('move every band below a taller one down by the extra rows', () => {
    expect(layerHeight(1)).toBe(LAYER_HEIGHT);
    expect(layerHeight(3)).toBe(LAYER_HEIGHT + 2 * ROW_HEIGHT);
    const heights = { ...SINGLE_ROW_HEIGHTS, trust: layerHeight(2) };
    expect(layerPosition('workloads', heights).y).toBe(layerHeight(2) + LAYER_GAP);
    expect(layerPosition('hardware', heights).y - layerPosition('hardware').y).toBe(ROW_HEIGHT);
    expect(layerAtY(LAYER_HEIGHT + 10, heights)).toBe('trust');
    expect(layerAtY(LAYER_HEIGHT + 10)).toBeNull();
  });

  it('validate ids and look up definitions', () => {
    expect(isLayerId('gpu')).toBe(true);
    expect(isLayerId('cloud')).toBe(false);
    expect(isLayerId(3)).toBe(false);
    expect(layerById('talos').label).toBe('Talos');
  });
});
