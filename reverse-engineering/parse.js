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
    imports: [],
    specificConstants: {
        qM: null,
        ZM: null,
        JM: null
    }
};

// Walk through the AST
function analyzeNode(node) {
    switch(node.type) {
        case 'VariableDeclaration':
            if (node.kind === 'const') {
                node.declarations.forEach(decl => {
                    const constName = decl.id.name;
                    const constInfo = {
                        name: constName,
                        type: decl.init?.type || 'undefined',
                        value: null
                    };

                    // Capture the actual value if it's a literal
                    if (decl.init?.type === 'Literal') {
                        constInfo.value = decl.init.value;
                    }
                    // For array literals
                    else if (decl.init?.type === 'ArrayExpression') {
                        constInfo.value = decl.init.elements.map(el =>
                            el.type === 'Literal' ? el.value : null
                        );
                    }
                    // For object literals
                    else if (decl.init?.type === 'ObjectExpression') {
                        constInfo.value = {};
                        decl.init.properties.forEach(prop => {
                            if (prop.value.type === 'Literal') {
                                constInfo.value[prop.key.name || prop.key.value] = prop.value.value;
                            }
                        });
                    }

                    analysis.constants.push(constInfo);

                    // Check if it's one of our specific constants
                    if (['qM', 'ZM', 'JM'].includes(constName)) {
                        analysis.specificConstants[constName] = constInfo.value;
                    }
                });
            }
            break;
        // ... rest of the cases remain the same
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

// Output specific constants first
console.log('=== Specific Constants Values ===');
for (const [name, value] of Object.entries(analysis.specificConstants)) {
    fs.writeFileSync(
        `./${name}.json`,
        JSON.stringify(value, null, 2),
        'utf8'
    );
}

// Rest of the output remains the same
// console.log('\n=== All Constants ===');
// analysis.constants.forEach(c => console.log(`${c.name}: ${c.type}${c.value !== null ? ` = ${JSON.stringify(c.value)}` : ''}`));
//
// console.log('\n=== Classes ===');
// analysis.classes.forEach(c => console.log(`${c.name}${c.superClass ? ` extends ${c.superClass}` : ''}`));
//
// console.log('\n=== Functions ===');
// analysis.functions.forEach(f => console.log(`${f.name}(${f.params.join(', ')})`));
//
// console.log('\n=== Imports ===');
// analysis.imports.forEach(i => console.log(`from ${i.source}: ${i.specifiers.join(', ')}`));