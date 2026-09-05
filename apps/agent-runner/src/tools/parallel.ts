/**
 * Parallel Partner Track Search API Client
 * Connects the VACT Cinema Agent to Parallel's Live Search API
 * for discovering cinematic b-roll, audio tracks, and editing recipes.
 */

export interface ParallelSearchResult {
  title: string;
  url: string;
  snippet: string;
  source?: string;
}

export interface ParallelSearchResponse {
  query: string;
  results: ParallelSearchResult[];
  totalResults?: number;
}

export class ParallelClient {
  private readonly _apiKey: string;
  private readonly _baseUrl: string;

  public constructor(
    apiKey = process.env['PARALLEL_API_KEY'] ?? '',
    baseUrl = process.env['PARALLEL_BASE_URL'] ?? 'https://api.parallelweb.com/v1'
  ) {
    this._apiKey = apiKey;
    this._baseUrl = baseUrl;
  }

  public get isConfigured(): boolean {
    return !!this._apiKey;
  }

  /**
   * Execute a search query on the Parallel Search API.
   * If no API key is provided, returns high-quality fallback cinema knowledge.
   */
  public async search(query: string, maxResults = 5): Promise<ParallelSearchResponse> {
    if (!this._apiKey) {
      return this._fallbackCinemaSearch(query);
    }

    try {
      const url = `${this._baseUrl}/search?q=${encodeURIComponent(query)}&limit=${maxResults}`;
      const res = await fetch(url, {
        method: 'GET',
        headers: {
          Authorization: `Bearer ${this._apiKey}`,
          'Content-Type': 'application/json',
          'User-Agent': 'VACT-Cinema-Agent/1.0',
        },
      });

      if (!res.ok) {
        throw new Error(`Parallel Search API returned ${res.status}: ${await res.text()}`);
      }

      const data = (await res.json()) as {
        results?: Array<{ title?: string; url?: string; snippet?: string; description?: string }>;
        total?: number;
      };

      const results: ParallelSearchResult[] = (data.results ?? []).map((r) => ({
        title: r.title ?? 'Cinema Asset',
        url: r.url ?? '',
        snippet: r.snippet ?? r.description ?? '',
        source: 'Parallel Search API',
      }));

      return {
        query,
        results,
        totalResults: data.total ?? results.length,
      };
    } catch (err) {
      console.warn(`[PARALLEL API WARNING] ${(err as Error).message}. Falling back to cinema index.`);
      return this._fallbackCinemaSearch(query);
    }
  }

  /**
   * Offline built-in cinema knowledge base for editing workflows and shortcuts.
   */
  private _fallbackCinemaSearch(query: string): ParallelSearchResponse {
    const q = query.toLowerCase();
    const mockResults: ParallelSearchResult[] = [];

    if (q.includes('davinci') || q.includes('color') || q.includes('resolve')) {
      mockResults.push(
        {
          title: 'DaVinci Resolve 19 Color Grading Node Graph Shortcuts',
          url: 'https://blackmagicdesign.com/davinci-resolve/shortcuts',
          snippet: 'Alt+S adds serial node; Alt+P adds parallel node. Teal and orange look achieved by offsetting gain toward cyan and lift toward orange with 35% saturation roll-off.',
          source: 'Cinema Knowledge Index',
        },
        {
          title: 'DaVinci Resolve Timeline Cut & Razor Tool',
          url: 'https://blackmagicdesign.com/davinci-resolve/edit',
          snippet: 'Ctrl+\\ splits clip at playhead. Ripple Delete with Shift+Backspace. Blade tool shortcut: B.',
          source: 'Cinema Knowledge Index',
        }
      );
    } else if (q.includes('blender') || q.includes('3d') || q.includes('camera')) {
      mockResults.push(
        {
          title: 'Blender 4.x Cinematic Camera Staging & Focal Lengths',
          url: 'https://docs.blender.org/manual/en/latest/render/cameras.html',
          snippet: 'Set camera sensor fit to 35mm full frame. 50mm for natural human perspective; 85mm for cinematic close-up portraits with shallow depth of field (f/1.8).',
          source: 'Cinema Knowledge Index',
        }
      );
    } else {
      mockResults.push({
        title: `Cinematic Asset Search: "${query}"`,
        url: 'https://vact.fy2ne.me/cinema-search',
        snippet: `Asset search index for "${query}" — 4K ProRes cinematic b-roll, LUT profiles, and audio stems ready for autonomous timeline placement.`,
        source: 'Parallel Partner Simulator',
      });
    }

    return {
      query,
      results: mockResults,
      totalResults: mockResults.length,
    };
  }
}
