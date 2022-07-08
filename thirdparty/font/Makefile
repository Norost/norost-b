inconsolata:: inconsolata/Inconsolata-VF.ttf inconsolata/OFL.txt

inconsolata/:
	mkdir $@

inconsolata/Inconsolata-VF.ttf inconsolata/OFL.txt: | inconsolata/
	wget https://github.com/googlefonts/Inconsolata/releases/download/v3.000/$(patsubst inconsolata/%,%,$@) -O $@
