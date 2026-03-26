import './style.css'

// 1. Terminal Typing Effect
const terminalLines = [
  '<span class="token-comment">// Agent: "I need to edit handle_auth. What depends on it?"</span>',
  '<span class="token-keyword">const</span> sym = <span class="token-function">prism.symbol</span>(<span class="token-string">"handle_auth"</span>);',
  '<span class="token-keyword">const</span> context = <span class="token-function">prism.readContext</span>(sym);',
  '',
  '<span class="token-comment">// PRISM returns:</span>',
  '<span class="token-keyword">return</span> {',
  '  <span class="token-string">"lineage"</span>: <span class="token-string">"active"</span>,',
  '  <span class="token-string">"coChangeNeighbors"</span>: [<span class="token-string">"auth_middleware"</span>, <span class="token-string">"session_store"</span>],',
  '  <span class="token-string">"recentFailures"</span>: [<span class="token-string">"test_token_expiry"</span>]',
  '};'
];

function typeWriter() {
  const termBody = document.getElementById('term-typing') as HTMLElement;
  if (!termBody) return;

  let lineIdx = 0;
  let charIdx = 0;
  let isTag = false;
  let text = '';

  function type() {
    if (lineIdx < terminalLines.length) {
      if (charIdx < terminalLines[lineIdx].length) {
        let char = terminalLines[lineIdx].charAt(charIdx);
        text += char;
        if (char === '<') isTag = true;
        if (char === '>') isTag = false;
        
        termBody.innerHTML = text + (isTag ? '' : '<span style="opacity:0.5">_</span>');
        
        charIdx++;
        let speed = isTag ? 0 : (Math.random() * 30 + 10);
        setTimeout(type, speed);
      } else {
        text += '<br>';
        lineIdx++;
        charIdx = 0;
        setTimeout(type, 300); // pause between lines
      }
    } else {
      termBody.innerHTML = text; // done
    }
  }

  // start typing delay
  setTimeout(type, 1000);
}

// 2. Intersection Observer for Fade-Ins
function initObservers() {
  const elements = document.querySelectorAll('.observe-fade');
  
  const observer = new IntersectionObserver((entries) => {
    entries.forEach(entry => {
      if (entry.isIntersecting) {
        entry.target.classList.add('visible');
        if (entry.target.querySelector('#term-typing')) {
          typeWriter();
          observer.unobserve(entry.target); // only type once
        }
      }
    });
  }, { threshold: 0.1 });

  elements.forEach(el => observer.observe(el));
}

// 3. Background Graph Canvas Animation
function initCanvas() {
  const canvas = document.getElementById('graph-canvas') as HTMLCanvasElement;
  if (!canvas) return;
  const ctx = canvas.getContext('2d');
  if (!ctx) return;

  let width = canvas.width = window.innerWidth;
  let height = canvas.height = window.innerHeight;

  window.addEventListener('resize', () => {
    width = canvas.width = window.innerWidth;
    height = canvas.height = window.innerHeight;
  });

  const nodes: {x: number, y: number, vx: number, vy: number}[] = [];
  const nodeCount = Math.floor(window.innerWidth / 15);

  for (let i = 0; i < nodeCount; i++) {
    nodes.push({
      x: Math.random() * width,
      y: Math.random() * height,
      vx: (Math.random() - 0.5) * 0.5,
      vy: (Math.random() - 0.5) * 0.5
    });
  }

  function draw() {
    ctx!.clearRect(0, 0, width, height);
    
    // update
    for (const node of nodes) {
      node.x += node.vx;
      node.y += node.vy;

      if (node.x < 0 || node.x > width) node.vx *= -1;
      if (node.y < 0 || node.y > height) node.vy *= -1;
    }

    // draw edges
    ctx!.lineWidth = 1;
    for (let i = 0; i < nodes.length; i++) {
      for (let j = i + 1; j < nodes.length; j++) {
        const dx = nodes[i].x - nodes[j].x;
        const dy = nodes[i].y - nodes[j].y;
        const dist = dx * dx + dy * dy;

        if (dist < 15000) {
          ctx!.beginPath();
          ctx!.moveTo(nodes[i].x, nodes[i].y);
          ctx!.lineTo(nodes[j].x, nodes[j].y);
          const opacity = 1 - (dist / 15000);
          ctx!.strokeStyle = `rgba(0, 240, 255, ${opacity * 0.15})`;
          ctx!.stroke();
        }
      }
    }

    // draw nodes
    ctx!.fillStyle = 'rgba(255, 0, 85, 0.4)'; // magenta hint
    for (const node of nodes) {
      ctx!.beginPath();
      ctx!.arc(node.x, node.y, 1.5, 0, Math.PI * 2);
      ctx!.fill();
    }

    requestAnimationFrame(draw);
  }

  draw();
}

document.addEventListener('DOMContentLoaded', () => {
  initObservers();
  initCanvas();
  // Hero is immediately visible
  document.querySelector('.hero')?.classList.add('visible');
});
