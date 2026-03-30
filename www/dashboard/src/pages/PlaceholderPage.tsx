type PlaceholderPageProps = {
  title: string
  eyebrow: string
  description: string
  highlights: string[]
  ctaLabel: string
  ctaPath: string
  focusLabel?: string | null
  onNavigate: (path: string) => void
}

export function PlaceholderPage({
  title,
  eyebrow,
  description,
  highlights,
  ctaLabel,
  ctaPath,
  focusLabel,
  onNavigate,
}: PlaceholderPageProps) {
  return (
    <div className="page-stack placeholder-layout">
      <section className="hero-bar panel page-hero">
        <div>
          <p className="eyebrow">{eyebrow}</p>
          <h2>{title}</h2>
          <p className="lede">{description}</p>
          {focusLabel ? <p className="focus-note">Focused from overview: {focusLabel}</p> : null}
        </div>
        <div className="hero-actions">
          <button type="button" className="ghost-button" onClick={() => onNavigate(ctaPath)}>
            {ctaLabel}
          </button>
        </div>
      </section>

      <section className="placeholder-grid">
        <article className="panel callout-card">
          <div className="panel-header">
            <h3>Implementation Notes</h3>
            <span>live route</span>
          </div>
          <div className="panel-body">
            <p>{description}</p>
          </div>
        </article>

        <article className="panel">
          <div className="panel-header">
            <h3>Next Focus</h3>
            <span>milestone</span>
          </div>
          <div className="panel-body placeholder-list">
            {highlights.map((item) => (
              <p key={item}>{item}</p>
            ))}
          </div>
        </article>
      </section>
    </div>
  )
}
