local mposx = 0.0;
local mposy = 0.0;
local hovered: function() = nil;
local cursorIndex = 1
local buttonWidth = 250.0;
local buttonHeight = 50.0;
local label = -1;

local gr_r, gr_g, gr_b, gr_a = game:GetSkinSetting("col_test")
gfx:GradientColors(0,127,255,255,0,128,255,0)
local gradient = gfx:LinearGradient(0,0,0,1)
local bgPattern = gfx:CreateSkinImage("bg_pattern.png", gfx.IMAGE_REPEATX + gfx.IMAGE_REPEATY)
local bgAngle = 0.5
local bgPaint = gfx:ImagePattern(0,0, 256,256, bgAngle, bgPattern, 1.0)
local bgPatternTimer = 0.0
local cursorYs = {}
local buttons: {number: {string, function()}} = nil
local resx, resy = game:GetResolution();

local function sign(x: number): integer
	return x>0 and 1 or x<0 and -1 or 0
end
 

local function mouse_clipped(x: number,y :number,w: number,h: number): boolean
    return mposx > x and mposy > y and mposx < x+w and mposy < y+h;
end;

local function draw_button(button: {string, function()}, x: number, y: number): number
	local name = button[1]
    local rx = x - (buttonWidth / 2);
    local ty = y - (buttonHeight / 2);
    gfx:BeginPath();
    gfx:TextAlign(gfx.TEXT_ALIGN_CENTER + gfx.TEXT_ALIGN_MIDDLE);
	
	gfx:FontSize(40);
	
	if mouse_clipped(rx,ty, buttonWidth, buttonHeight) then
       hovered = button[2];
	   local b_r, b_g, b_b, b_a = game:GetSkinSetting("col_test")
       gfx:FillColor(b_r,b_g,b_b);
	   gfx:Text(name, x+1, y+1);
	   gfx:Text(name, x-1, y+1);
	   gfx:Text(name, x+1, y-1);
	   gfx:Text(name, x-1, y-1);
    end
	gfx:FillColor(255,255,255);
    gfx:Text(name, x, y);
	return buttonHeight + 5
end;

local function updateGradient()
	gr_r, gr_g, gr_b, gr_a = game:GetSkinSetting("col_test")
	if gr_r == nil then return end
	gfx:GradientColors(gr_r,gr_g,gr_b,gr_a,0,128,255,0)
	--gradient = gfx:LinearGradient(0,0,0,1)
end

local function updatePattern(dt: number)
	bgPatternTimer = (bgPatternTimer + dt) % 1.0
	local bgx = math.cos(bgAngle) * (bgPatternTimer * 256)
	local bgy = math.sin(bgAngle) * (bgPatternTimer * 256)
	gfx:UpdateImagePattern(bgPaint, bgx, bgy, 256, 256, bgAngle, 1.0)
end

local function setButtons()
	if buttons == nil then
		buttons = {}
		buttons[1] = {"Start", updateGradient}
		buttons[2] = {"Multiplayer", updateGradient}
        buttons[3] = {"Challenges", updateGradient}
		buttons[4] = {"Get Songs", updateGradient}
		buttons[5] = {"Settings", updateGradient}
		buttons[6] = {"Exit", updateGradient}
	end
end

local renderY = resy/2
local function draw_cursor(x: number,y:number,deltaTime:number)
	gfx:Save()
	gfx:BeginPath()
	
	local size = 8

	gfx:BeginPath()

	gfx:BeginPath()
	gfx:MoveTo(2,5)
	gfx:Fill()
	
	renderY = renderY - (renderY - y) * deltaTime * 30
	
	gfx:MoveTo(x-size,renderY-size)
	gfx:LineTo(x,renderY)
	gfx:LineTo(x-size,renderY+size)
	
	gfx:StrokeWidth(3)
	gfx:StrokeColor(255,255,255)
	gfx:Stroke()
	
	gfx:Restore()
end




local function roundToZero(x: number): integer
	if x<0 then return math.ceil(x)
	elseif x>0 then return math.floor(x)
	else return 0 end
end

local function deltaKnob(delta : number): number
	if math.abs(delta) > 1.5 * math.pi then 
		return delta + 2 * math.pi * sign(delta) * -1
	end
	return delta
end



local lastKnobs: {number: number} = nil
local knobProgress = 0.0
local function handle_controller()
	if lastKnobs == nil then
		lastKnobs = {game:GetKnob(0), game:GetKnob(1)}
	else
		local newKnobs = {game:GetKnob(0), game:GetKnob(1)}
	
		knobProgress = knobProgress - deltaKnob(lastKnobs[1] - newKnobs[1]) * 1.2
		knobProgress = knobProgress - deltaKnob(lastKnobs[2] - newKnobs[2]) * 1.2
		
		lastKnobs = newKnobs
		
		if math.abs(knobProgress) > 1 then
			cursorIndex = (((cursorIndex - 1) + roundToZero(knobProgress)) % #buttons) + 1
			knobProgress = knobProgress - roundToZero(knobProgress)
		end
	end
end

global function render(deltaTime: number)
	setButtons()
	updateGradient()
	updatePattern(deltaTime)
    resx,resy = game:GetResolution();
    mposx,mposy = game:GetMousePos();
    gfx:Scale(resx, resy / 3)
    gfx:Rect(0,0,1,1)
    gfx:FillPaint(gradient)
    gfx:Fill()
    gfx:ResetTransform()
    gfx:BeginPath()
	gfx:Scale(0.5,0.5)
	gfx:Rect(0,0,resx * 2,resy * 2)
	gfx:GlobalCompositeOperation(gfx.BLEND_OP_DESTINATION_IN)
    gfx:FillPaint(bgPaint)
    gfx:Fill()
	gfx:ResetTransform()
	gfx:BeginPath()
	gfx:GlobalCompositeOperation(gfx.BLEND_OP_SOURCE_OVER)
	
    local buttonY = resy / 2;
    hovered = nil;
	
    gfx:LoadSkinFont("NotoSans-Regular.ttf");
	
	for i=1,#buttons do
		cursorYs[i] = buttonY
		buttonY = buttonY + draw_button(buttons[i], resx / 2, buttonY);
		if hovered == buttons[i][2] then
			cursorIndex = i
		end
	end
	
	handle_controller()
	
	draw_cursor(resx/2 - 100, cursorYs[cursorIndex], deltaTime)
	
    gfx:BeginPath();
    gfx:FillColor(255,255,255);
    gfx:FontSize(120);
    if label == -1 then
        label = gfx:CreateLabel("unnamed_sdvx_clone", 120, false);
    end
    gfx:TextAlign(gfx.TEXT_ALIGN_CENTER + gfx.TEXT_ALIGN_MIDDLE);
    gfx:DrawLabel(label, resx / 2, resy / 2 - 200, resx-40);

end;

global function mouse_pressed(button: integer): number
    if hovered then
        hovered()
    end
    return 0
end

global function button_pressed(button: integer)
    if button == game.BUTTON_STA then 
        buttons[cursorIndex][2]()
    elseif button == game.BUTTON_BCK then
        
    end
end
