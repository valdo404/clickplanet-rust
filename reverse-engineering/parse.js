const fs = require('fs');
const acorn = require('acorn');

const content = fs.readFileSync('./script.js', 'utf8');
const ast = acorn.parse(content, {
    ecmaVersion: 'latest',
    sourceType: 'module'
});

const analysis = {
    constants: [],
    classes: [],
    functions: [],
    imports: []
};

// Walk through the AST
function analyzeNode(node) {
    switch(node.type) {
        case 'VariableDeclaration':
            if (node.kind === 'const') {
                node.declarations.forEach(decl => {
                    analysis.constants.push({
                        name: decl.id.name,
                        type: decl.init?.type || 'undefined'
                    });
                });
            }
            break;
        case 'ClassDeclaration':
            analysis.classes.push({
                name: node.id.name,
                superClass: node.superClass?.name
            });
            break;
        case 'FunctionDeclaration':
            analysis.functions.push({
                name: node.id.name,
                params: node.params.map(p => p.name)
            });
            break;
        case 'ImportDeclaration':
            analysis.imports.push({
                source: node.source.value,
                specifiers: node.specifiers.map(s => s.local.name)
            });
            break;
    }

    // Recursively analyze child nodes
    for (let key in node) {
        if (node[key] && typeof node[key] === 'object') {
            if (Array.isArray(node[key])) {
                node[key].forEach(child => {
                    if (child && typeof child === 'object') {
                        analyzeNode(child);
                    }
                });
            } else {
                analyzeNode(node[key]);
            }
        }
    }
}

analyzeNode(ast);

// Output results in a structured way
console.log('=== Constants ===');
analysis.constants.forEach(c => console.log(`${c.name}: ${c.type}`));

console.log('\n=== Classes ===');
analysis.classes.forEach(c => console.log(`${c.name}${c.superClass ? ` extends ${c.superClass}` : ''}`));

console.log('\n=== Functions ===');
analysis.functions.forEach(f => console.log(`${f.name}(${f.params.join(', ')})`));

console.log('\n=== Imports ===');
analysis.imports.forEach(i => console.log(`from ${i.source}: ${i.specifiers.join(', ')}`));
